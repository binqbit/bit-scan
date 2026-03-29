#[cfg(feature = "cuda")]
mod cuda {
    use std::{
        env,
        error::Error,
        path::{Path, PathBuf},
        sync::OnceLock,
        thread,
        time::{Duration, Instant},
    };

    use cudarc::{
        driver::{CudaContext, LaunchConfig, PushKernelArg},
        nvrtc::compile_ptx,
    };
    use libloading::Library;
    use rand::Rng;
    use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

    use crate::utils::{
        extract_hash160_from_base58_address, hash160, number_to_private_key,
        private_to_compressed_pubkey, save_private_key_to_file,
    };

    const FUNC_NAME: &str = "fill_randoms";
    const DEFAULT_BATCH_SIZE: usize = 262_144;
    const DEFAULT_BLOCK_SIZE: u32 = 256;
    const DEFAULT_ITEMS_PER_THREAD: u32 = 64;
    const MAX_BLOCK_SIZE: u32 = 1024;
    const MAX_ITEMS_PER_THREAD: u32 = 256;
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
    unsigned int items_per_thread,
    unsigned long long *out_hi,
    unsigned long long *out_lo,
    unsigned long long count
) {
    unsigned long long logical_thread = blockIdx.x * blockDim.x + threadIdx.x;
    unsigned long long start = logical_thread * items_per_thread;

    unsigned int lo_bits = bits >= 64u ? 64u : bits;
    unsigned int hi_bits = bits > 64u ? bits - 64u : 0u;

    for (unsigned int item = 0u; item < items_per_thread; ++item) {
        unsigned long long idx = start + item;
        if (idx >= count) {
            return;
        }

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
}
"#;

    #[derive(Clone, Copy)]
    struct ScanConfig {
        batch_size: usize,
        block_size: u32,
        items_per_thread: u32,
        verify_threads: usize,
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
            let threads = thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .max(1);
            crate::scan_v4::scan(pubkey, bits, stats, threads);
        }
    }

    fn scan_with_cuda(pubkey: &str, bits: u32, stats: bool) -> Result<(), Box<dyn Error>> {
        let config = ScanConfig::from_env();
        let pubkey_hash = extract_hash160_from_base58_address(pubkey);

        preload_cuda_runtime()?;

        let ctx = CudaContext::new(0)?;
        let stream = ctx.default_stream();
        let ptx = compile_ptx(KERNEL_SRC)?;
        let module = ctx.load_module(ptx)?;
        let func = module.load_function(FUNC_NAME)?;

        let verify_pool = build_verifier_pool(config.verify_threads)?;

        let mut d_hi = stream.alloc_zeros::<u64>(config.batch_size)?;
        let mut d_lo = stream.alloc_zeros::<u64>(config.batch_size)?;
        let mut host_hi = vec![0u64; config.batch_size];
        let mut host_lo = vec![0u64; config.batch_size];

        let mut rng = rand::thread_rng();
        let mut seed = rng.r#gen::<u64>() | 1;
        let cfg = config.launch_config();

        let mut total_candidates: u64 = 0;
        let mut window_candidates: u64 = 0;
        let mut last_report = Instant::now();

        loop {
            seed = seed.wrapping_add(SEED_INCREMENT);

            {
                let count = config.batch_size as u64;
                let mut builder = stream.launch_builder(&func);
                builder
                    .arg(&seed)
                    .arg(&bits)
                    .arg(&config.items_per_thread)
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
                        let num = ((hi as u128) << 64) | (lo as u128);
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

    impl ScanConfig {
        fn from_env() -> Self {
            let logical_cpus = thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .max(1);
            let batch_size = parse_env_usize("BIT_SCAN_V3_BATCH_SIZE")
                .unwrap_or(DEFAULT_BATCH_SIZE)
                .max(1);
            let block_size = parse_env_u32("BIT_SCAN_V3_BLOCK_SIZE")
                .unwrap_or(DEFAULT_BLOCK_SIZE)
                .clamp(32, MAX_BLOCK_SIZE);
            let items_per_thread = parse_env_u32("BIT_SCAN_V3_ITEMS_PER_THREAD")
                .unwrap_or(DEFAULT_ITEMS_PER_THREAD)
                .clamp(1, MAX_ITEMS_PER_THREAD);
            let verify_threads = parse_env_usize("BIT_SCAN_V3_VERIFY_THREADS")
                .unwrap_or(logical_cpus)
                .clamp(1, logical_cpus);

            Self {
                batch_size,
                block_size,
                items_per_thread,
                verify_threads,
            }
        }

        fn launch_config(&self) -> LaunchConfig {
            let logical_threads = (self.batch_size as u64)
                .div_ceil(self.items_per_thread as u64)
                .clamp(1, u32::MAX as u64) as u32;
            let grid_x = logical_threads.div_ceil(self.block_size);
            LaunchConfig {
                grid_dim: (grid_x.max(1), 1, 1),
                block_dim: (self.block_size, 1, 1),
                shared_mem_bytes: 0,
            }
        }
    }

    fn parse_env_usize(key: &str) -> Option<usize> {
        env::var(key).ok()?.parse().ok()
    }

    fn parse_env_u32(key: &str) -> Option<u32> {
        env::var(key).ok()?.parse().ok()
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
