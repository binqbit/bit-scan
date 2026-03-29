#[cfg(feature = "cuda")]
mod cuda {
    use std::{
        error::Error,
        path::{Path, PathBuf},
        sync::OnceLock,
        time::{Duration, Instant},
    };

    use cudarc::{
        driver::{CudaContext, LaunchConfig, PushKernelArg},
        nvrtc::compile_ptx,
    };
    use libloading::Library;
    use rand::Rng;
    use rayon::prelude::*;

    use crate::utils::{
        extract_hash160_from_base58_address, hash160, number_to_private_key,
        private_to_compressed_pubkey, save_private_key_to_file,
    };

    const FUNC_NAME: &str = "fill_randoms";
    const BATCH_SIZE: usize = 262_144;
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
    unsigned long long idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= count) {
        return;
    }

    unsigned long long state = seed ^ ((idx + 1ull) * 0x9E3779B97F4A7C15ull);

    unsigned long long lo = xorshift64(&state);
    unsigned long long hi = xorshift64(&state);

    unsigned int lo_bits = bits >= 64u ? 64u : bits;
    unsigned int hi_bits = bits > 64u ? bits - 64u : 0u;

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

    pub fn scan(pubkey: &str, bits: u32, stats: bool) {
        assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

        if let Err(err) = scan_with_cuda(pubkey, bits, stats) {
            eprintln!(
                "scan_v3: CUDA path unavailable ({err}). Falling back to version 1 engine..."
            );
            crate::scan_v1::scan(pubkey, bits, stats);
        }
    }

    fn scan_with_cuda(pubkey: &str, bits: u32, stats: bool) -> Result<(), Box<dyn Error>> {
        let pubkey_hash = extract_hash160_from_base58_address(pubkey);

        preload_cuda_runtime()?;

        let ctx = CudaContext::new(0)?;
        let stream = ctx.default_stream();
        let ptx = compile_ptx(KERNEL_SRC)?;
        let module = ctx.load_module(ptx)?;
        let func = module.load_function(FUNC_NAME)?;

        let mut d_hi = stream.alloc_zeros::<u64>(BATCH_SIZE)?;
        let mut d_lo = stream.alloc_zeros::<u64>(BATCH_SIZE)?;
        let mut host_hi = vec![0u64; BATCH_SIZE];
        let mut host_lo = vec![0u64; BATCH_SIZE];

        let mut rng = rand::thread_rng();
        let mut seed = rng.r#gen::<u64>() | 1;

        let cfg = LaunchConfig::for_num_elems(BATCH_SIZE as u32);
        let mut total_candidates: u64 = 0;
        let mut window_candidates: u64 = 0;
        let mut last_report = Instant::now();

        loop {
            seed = seed.wrapping_add(SEED_INCREMENT);

            {
                let count = BATCH_SIZE as u64;
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

            let result = host_hi
                .par_iter()
                .zip(host_lo.par_iter())
                .find_map_any(|(&hi, &lo)| {
                    let num = ((hi as u128) << 64) | (lo as u128);
                    let private_key = number_to_private_key(num);
                    let public_key = private_to_compressed_pubkey(&private_key);
                    let derived_pubkey = hash160(&public_key);
                    if derived_pubkey == pubkey_hash {
                        Some(private_key)
                    } else {
                        None
                    }
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

    fn preload_cuda_runtime() -> Result<(), Box<dyn Error>> {
        static INIT: OnceLock<Result<(), String>> = OnceLock::new();

        INIT.get_or_init(|| unsafe {
            let driver = [
                PathBuf::from("/run/opengl-driver/lib/libcuda.so.1"),
                PathBuf::from("/run/opengl-driver/lib/libcuda.so"),
            ]
            .into_iter()
            .find(|path| path.exists())
            .ok_or_else(|| "real NVIDIA driver library not found in /run/opengl-driver/lib".to_string())?;
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
        })
        .as_ref()
        .copied()
        .map_err(|err| err.clone().into())
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
            if let Some(root) = std::env::var_os(key).map(PathBuf::from) {
                if has_nvrtc(&root) {
                    return Some(root);
                }
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
    eprintln!("scan_v3: binary built without CUDA support. Falling back to version 1 engine...");
    crate::scan_v1::scan(pubkey, bits, stats);
}
