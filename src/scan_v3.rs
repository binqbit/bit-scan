#[cfg(feature = "cuda")]
mod cuda {
    use std::{
        error::Error,
        ffi::c_int,
        path::{Path, PathBuf},
        sync::{Arc, OnceLock},
        thread,
        time::{Duration, Instant},
    };

    use cudarc::{
        driver::{CudaContext, CudaFunction, LaunchConfig, PushKernelArg, sys::CUdevice_attribute},
        nvrtc::compile_ptx,
    };
    use libloading::Library;
    use rand::Rng;
    use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

    use crate::utils::{
        extract_hash160_from_base58_address, hash160, normalize_number_to_bit_length,
        number_to_private_key, private_to_compressed_pubkey, save_private_key_to_file,
    };

    const FUNC_NAME: &str = "fill_randoms";
    const SEED_INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;

    const KERNEL_SRC: &str = r#"
extern "C" __device__ __forceinline__ unsigned long long xorshift64(unsigned long long *state) {
    unsigned long long x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    return x * 2685821657736338717ull;
}

extern "C" __global__ void fill_randoms(
    unsigned long long seed,
    unsigned int bits,
    unsigned long long *out_hi,
    unsigned long long *out_lo,
    unsigned long long count
) {
    unsigned long long idx =
        (unsigned long long)blockIdx.x * blockDim.x + threadIdx.x;

    if (idx >= count) {
        return;
    }

    unsigned int lo_bits = bits >= 64u ? 64u : bits;
    unsigned int hi_bits = bits > 64u ? bits - 64u : 0u;

    unsigned long long state = seed ^ ((idx + 1ull) * 0x9E3779B97F4A7C15ull);

    unsigned long long lo = xorshift64(&state);
    unsigned long long hi = xorshift64(&state);

    if (lo_bits < 64u) {
        unsigned long long mask = (1ull << lo_bits) - 1ull;
        lo &= mask;
    }

    if (hi_bits == 0u) {
        hi = 0ull;
    } else if (hi_bits < 64u) {
        unsigned long long mask = (1ull << hi_bits) - 1ull;
        hi &= mask;
    }

    if (bits <= 64u) {
        unsigned int shift = bits - 1u;
        lo |= (1ull << shift);
    } else {
        unsigned int shift = hi_bits - 1u;
        hi |= (1ull << shift);
    }

    out_hi[idx] = hi;
    out_lo[idx] = lo;
}
"#;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CudaLaunchShape {
        grid_x: u32,
        block_x: u32,
        candidate_count: usize,
    }

    pub fn scan(pubkey: &str, bits: u32, stats: bool) {
        assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

        if let Err(err) = crate::scan_v3_opencl::scan(pubkey, bits, stats) {
            eprintln!("scan_v3: OpenCL full-GPU path unavailable ({err}). Trying CUDA fallback...");
        } else {
            return;
        }

        if let Err(err) = scan_with_cuda(pubkey, bits, stats) {
            eprintln!(
                "scan_v3: CUDA path unavailable ({err}). Falling back to version 4 engine..."
            );
            crate::scan_v4::scan(pubkey, bits, stats, available_cpu_threads());
        }
    }

    fn scan_with_cuda(pubkey: &str, bits: u32, stats: bool) -> Result<(), Box<dyn Error>> {
        let pubkey_hash = extract_hash160_from_base58_address(pubkey);

        preload_cuda_runtime()?;

        let ctx = select_cuda_context()?;
        let stream = ctx.default_stream();
        let ptx = compile_ptx(KERNEL_SRC)?;
        let module = ctx.load_module(ptx)?;
        let func = module.load_function(FUNC_NAME)?;
        let launch_shape = cuda_launch_shape(&ctx, &func)?.limited_to_keyspace(bits);
        let verify_threads = available_cpu_threads();

        let verify_pool = build_verifier_pool(verify_threads)?;

        let mut d_hi = stream.alloc_zeros::<u64>(launch_shape.candidate_count)?;
        let mut d_lo = stream.alloc_zeros::<u64>(launch_shape.candidate_count)?;
        let mut host_hi = vec![0u64; launch_shape.candidate_count];
        let mut host_lo = vec![0u64; launch_shape.candidate_count];

        let mut rng = rand::thread_rng();
        let mut seed = rng.r#gen::<u64>() | 1;
        let cfg = launch_shape.launch_config();

        if stats {
            let device_name = ctx
                .name()
                .unwrap_or_else(|_| "unknown CUDA GPU".to_string());
            println!(
                "scan_v3: CUDA auto-selected {device_name}; grid {}, block {}, batch {}, CPU verifiers {}",
                launch_shape.grid_x,
                launch_shape.block_x,
                launch_shape.candidate_count,
                verify_threads
            );
        }

        let mut total_candidates: u64 = 0;
        let mut window_candidates: u64 = 0;
        let mut last_report = Instant::now();

        loop {
            seed = seed.wrapping_add(SEED_INCREMENT);

            {
                let count = launch_shape.candidate_count as u64;
                let mut builder = stream.launch_builder(&func);
                builder
                    .arg(&seed)
                    .arg(&bits)
                    .arg(&mut d_hi)
                    .arg(&mut d_lo)
                    .arg(&count);
                unsafe {
                    builder.launch(cfg)?;
                }
            }

            stream.memcpy_dtoh(&d_hi, host_hi.as_mut_slice())?;
            stream.memcpy_dtoh(&d_lo, host_lo.as_mut_slice())?;
            stream.synchronize()?;

            let batch_len = host_hi.len() as u64;
            total_candidates += batch_len;
            window_candidates += batch_len;

            if stats {
                let elapsed = last_report.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    let secs = elapsed.as_secs_f64();
                    if secs > 0.0 {
                        let rate = window_candidates as f64 / secs;
                        println!(
                            "Hashes: {:.2} per second (total processed {})",
                            rate, total_candidates
                        );
                    }
                    window_candidates = 0;
                    last_report = Instant::now();
                }
            }

            let result = verify_pool.install(|| {
                host_hi
                    .par_iter()
                    .zip(host_lo.par_iter())
                    .find_map_any(|(&hi, &lo)| {
                        let num = normalize_number_to_bit_length(
                            ((hi as u128) << 64) | (lo as u128),
                            bits,
                        );
                        let private_key = number_to_private_key(num);
                        let public_key = private_to_compressed_pubkey(&private_key);
                        let derived_pubkey = hash160(&public_key);
                        (derived_pubkey == pubkey_hash).then_some(private_key)
                    })
            });

            if let Some(private_key) = result {
                if stats && window_candidates > 0 {
                    let secs = last_report.elapsed().as_secs_f64();
                    if secs > 0.0 {
                        let rate = window_candidates as f64 / secs;
                        println!(
                            "Hashes: {:.2} per second (total processed {})",
                            rate, total_candidates
                        );
                    } else {
                        println!("Hashes: total processed {}", total_candidates);
                    }
                }

                println!("Match found! Private key: {}", hex::encode(private_key));
                save_private_key_to_file(pubkey, private_key, "found_keys")
                    .expect("Failed to save private key");

                return Ok(());
            }
        }
    }

    fn build_verifier_pool(
        verify_threads: usize,
    ) -> Result<ThreadPool, rayon::ThreadPoolBuildError> {
        ThreadPoolBuilder::new()
            .num_threads(verify_threads)
            .thread_name(|idx| format!("scan-v3-verify-{idx}"))
            .build()
    }

    impl CudaLaunchShape {
        fn new(
            grid_x: u32,
            block_x: u32,
            max_grid_x: u32,
            max_block_x: u32,
        ) -> Result<Self, String> {
            if grid_x == 0 || block_x == 0 {
                return Err("CUDA occupancy returned a zero launch dimension".to_string());
            }
            if grid_x > max_grid_x {
                return Err(format!(
                    "CUDA occupancy grid {grid_x} exceeds device limit {max_grid_x}"
                ));
            }
            if block_x > max_block_x {
                return Err(format!(
                    "CUDA occupancy block {block_x} exceeds device limit {max_block_x}"
                ));
            }

            let candidate_count = usize::try_from(u64::from(grid_x) * u64::from(block_x))
                .map_err(|_| "CUDA launch size does not fit usize".to_string())?;

            Ok(Self {
                grid_x,
                block_x,
                candidate_count,
            })
        }

        fn launch_config(self) -> LaunchConfig {
            LaunchConfig {
                grid_dim: (self.grid_x, 1, 1),
                block_dim: (self.block_x, 1, 1),
                shared_mem_bytes: 0,
            }
        }

        fn limited_to_keyspace(mut self, bits: u32) -> Self {
            let keyspace_size = 1u128 << (bits - 1);
            self.candidate_count = (self.candidate_count as u128).min(keyspace_size) as usize;
            self
        }
    }

    extern "C" fn zero_dynamic_smem(_: c_int) -> usize {
        0
    }

    fn cuda_launch_shape(
        ctx: &CudaContext,
        func: &CudaFunction,
    ) -> Result<CudaLaunchShape, Box<dyn Error>> {
        let (grid_x, block_x) =
            func.occupancy_max_potential_block_size(zero_dynamic_smem, 0, 0, None)?;
        let max_grid_x = positive_cuda_attribute(
            ctx.attribute(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MAX_GRID_DIM_X)?,
            "maximum grid dimension",
        )?;
        let max_block_x = positive_cuda_attribute(
            ctx.attribute(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MAX_THREADS_PER_BLOCK)?,
            "maximum threads per block",
        )?;

        CudaLaunchShape::new(grid_x, block_x, max_grid_x, max_block_x).map_err(Into::into)
    }

    fn select_cuda_context() -> Result<Arc<CudaContext>, Box<dyn Error>> {
        let device_count = usize::try_from(CudaContext::device_count()?)
            .map_err(|_| "CUDA reported a negative device count")?;
        if device_count == 0 {
            return Err("no CUDA devices found".into());
        }

        let mut best: Option<(u64, Arc<CudaContext>)> = None;
        let mut last_error = None;

        for ordinal in 0..device_count {
            match CudaContext::new(ordinal).and_then(|ctx| {
                let compute_units =
                    ctx.attribute(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT)?;
                let clock_rate =
                    ctx.attribute(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_CLOCK_RATE)?;
                Ok((ctx, compute_units, clock_rate))
            }) {
                Ok((ctx, compute_units, clock_rate)) => {
                    let compute_units = u64::from(positive_cuda_attribute(
                        compute_units,
                        "multiprocessor count",
                    )?);
                    let clock_rate =
                        u64::from(positive_cuda_attribute(clock_rate, "GPU clock rate")?);
                    let score = compute_units * clock_rate;

                    if best
                        .as_ref()
                        .is_none_or(|(best_score, _)| score > *best_score)
                    {
                        best = Some((score, ctx));
                    }
                }
                Err(err) => last_error = Some(err.to_string()),
            }
        }

        let (_, ctx) = best.ok_or_else(|| {
            last_error.unwrap_or_else(|| "no queryable CUDA device found".to_string())
        })?;
        ctx.bind_to_thread()?;
        Ok(ctx)
    }

    fn positive_cuda_attribute(value: i32, name: &str) -> Result<u32, String> {
        u32::try_from(value)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("CUDA reported invalid {name}: {value}"))
    }

    fn available_cpu_threads() -> usize {
        thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .max(1)
    }

    fn preload_cuda_runtime() -> Result<(), Box<dyn Error>> {
        static INIT: OnceLock<Result<(), String>> = OnceLock::new();

        match INIT.get_or_init(|| unsafe {
            let driver = [
                PathBuf::from("/run/opengl-driver/lib/libcuda.so.1"),
                PathBuf::from("/run/opengl-driver/lib/libcuda.so"),
            ]
            .into_iter()
            .find(|path| path.exists())
            .ok_or_else(|| {
                "real NVIDIA driver library not found in /run/opengl-driver/lib".to_string()
            })?;
            let driver_lib = Library::new(&driver)
                .map_err(|err| format!("failed to load CUDA driver {}: {err}", driver.display()))?;

            let nvrtc = cuda_root()
                .map(|root| root.join("lib/libnvrtc.so"))
                .filter(|path| path.exists())
                .ok_or_else(|| "libnvrtc.so not found in detected CUDA toolkit root".to_string())?;
            let nvrtc_lib = Library::new(&nvrtc)
                .map_err(|err| format!("failed to load NVRTC {}: {err}", nvrtc.display()))?;

            std::mem::forget(driver_lib);
            std::mem::forget(nvrtc_lib);
            Ok(())
        }) {
            Ok(()) => Ok(()),
            Err(err) => Err(err.clone().into()),
        }
    }

    fn cuda_root() -> Option<PathBuf> {
        for key in [
            "BIT_SCAN_CUDA_ROOT",
            "CUDA_ROOT",
            "CUDA_PATH",
            "CUDA_HOME",
            "CUDAToolkit_ROOT",
            "CUDA_TOOLKIT_ROOT_DIR",
        ] {
            if let Some(root) = std::env::var_os(key).map(PathBuf::from)
                && has_nvrtc(&root)
            {
                return Some(root);
            }
        }

        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".nix-profile"))
            .filter(|root| has_nvrtc(root))
            .or_else(|| {
                [
                    PathBuf::from("/run/current-system/sw"),
                    PathBuf::from("/usr/local/cuda"),
                    PathBuf::from("/opt/cuda"),
                ]
                .into_iter()
                .find(|root| has_nvrtc(root))
            })
    }

    fn has_nvrtc(root: &Path) -> bool {
        root.join("lib/libnvrtc.so").exists()
    }

    #[cfg(test)]
    mod tests {
        use super::{CudaLaunchShape, positive_cuda_attribute};

        #[test]
        fn cuda_launch_shape_uses_occupancy_dimensions() {
            let shape = CudaLaunchShape::new(160, 256, 1_000, 1_024).unwrap();

            assert_eq!(shape.candidate_count, 40_960);
            let launch = shape.launch_config();
            assert_eq!(launch.grid_dim, (160, 1, 1));
            assert_eq!(launch.block_dim, (256, 1, 1));
        }

        #[test]
        fn cuda_batch_is_capped_to_the_requested_keyspace() {
            let shape = CudaLaunchShape::new(160, 256, 1_000, 1_024).unwrap();

            assert_eq!(shape.limited_to_keyspace(1).candidate_count, 1);
            assert_eq!(shape.limited_to_keyspace(8).candidate_count, 128);
            assert_eq!(shape.limited_to_keyspace(128), shape);
        }

        #[test]
        fn cuda_launch_shape_rejects_invalid_dimensions() {
            assert!(CudaLaunchShape::new(0, 256, 1_000, 1_024).is_err());
            assert!(CudaLaunchShape::new(160, 0, 1_000, 1_024).is_err());
            assert!(CudaLaunchShape::new(1_001, 256, 1_000, 1_024).is_err());
            assert!(CudaLaunchShape::new(160, 1_025, 1_000, 1_024).is_err());
        }

        #[test]
        fn cuda_attributes_must_be_positive() {
            assert_eq!(positive_cuda_attribute(32, "test").unwrap(), 32);
            assert!(positive_cuda_attribute(0, "test").is_err());
            assert!(positive_cuda_attribute(-1, "test").is_err());
        }
    }
}

#[cfg(feature = "cuda")]
pub use cuda::scan;

#[cfg(not(feature = "cuda"))]
pub fn scan(pubkey: &str, bits: u32, stats: bool) {
    if let Err(err) = crate::scan_v3_opencl::scan(pubkey, bits, stats) {
        eprintln!(
            "scan_v3: OpenCL full-GPU path unavailable ({err}). Falling back to version 4 engine..."
        );
    } else {
        return;
    }

    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1);
    crate::scan_v4::scan(pubkey, bits, stats, threads);
}
