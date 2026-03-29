use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};

use opencl3::{
    command_queue::{CL_QUEUE_PROFILING_ENABLE, CommandQueue},
    context::Context,
    device::{CL_DEVICE_TYPE_GPU, Device, get_all_devices},
    kernel::{ExecuteKernel, Kernel},
    memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE},
    program::Program,
    types::{CL_BLOCKING, cl_uint},
};
use rand::Rng;

use crate::utils::{extract_hash160_from_base58_address, save_private_key_to_file};

const DEFAULT_BATCH_SIZE: usize = 262_144;
const DEFAULT_LOOP_COUNT: u32 = 8;
const DEFAULT_WORK_GROUP_SIZE: usize = 256;
const MAX_KERNEL_SPAN: usize = u32::MAX as usize;
const KERNEL_NAME: &str = "bit_scan_match_kernel";

#[derive(Clone, Copy)]
struct OpenClScanConfig {
    batch_size: usize,
    loop_count: u32,
    work_group_size: usize,
    device_index: usize,
}

pub fn scan(pubkey: &str, bits: u32, stats: bool) -> Result<(), Box<dyn Error>> {
    assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

    ensure_opencl_runtime_path();

    let config = OpenClScanConfig::from_env()?;
    let target_hash = extract_hash160_from_base58_address(pubkey);

    let device_id = *get_all_devices(CL_DEVICE_TYPE_GPU)?
        .get(config.device_index)
        .ok_or_else(|| format!("OpenCL GPU device {} not found", config.device_index))?;
    let device = Device::new(device_id);
    let context = Context::from_device(&device)?;
    let queue =
        CommandQueue::create_default_with_properties(&context, CL_QUEUE_PROFILING_ENABLE, 0)?;

    let vendor_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/opencl");
    let kernel_source = load_opencl_source(&vendor_root)?;
    let build_opts = "-cl-std=CL1.2";
    let program = Program::create_and_build_from_source(&context, &kernel_source, &build_opts)
        .map_err(|err| format!("OpenCL program build failed: {err}"))?;
    let kernel = Kernel::create(&program, KERNEL_NAME)?;

    let mut base_key_buffer =
        unsafe { Buffer::<cl_uint>::create(&context, CL_MEM_READ_ONLY, 8, ptr::null_mut())? };
    let mut target_hash_buffer = unsafe {
        Buffer::<u8>::create(
            &context,
            CL_MEM_READ_ONLY,
            target_hash.len(),
            ptr::null_mut(),
        )?
    };
    let mut found_flag_buffer =
        unsafe { Buffer::<cl_uint>::create(&context, CL_MEM_READ_WRITE, 1, ptr::null_mut())? };
    let found_key_buffer =
        unsafe { Buffer::<cl_uint>::create(&context, CL_MEM_READ_WRITE, 8, ptr::null_mut())? };

    unsafe {
        queue.enqueue_write_buffer(&mut target_hash_buffer, CL_BLOCKING, 0, &target_hash, &[])?;
    }

    let mut rng = rand::thread_rng();
    let mut total_candidates: u64 = 0;
    let mut window_candidates: u64 = 0;
    let mut last_report = Instant::now();
    let work_items = config.work_items();
    let zero_flag = [0u32];

    loop {
        let base = sample_batch_base(bits, config.batch_size as u32, &mut rng);
        let base_words = u128_to_le_words(base);

        unsafe {
            queue.enqueue_write_buffer(&mut base_key_buffer, CL_BLOCKING, 0, &base_words, &[])?;
            queue.enqueue_write_buffer(&mut found_flag_buffer, CL_BLOCKING, 0, &zero_flag, &[])?;
        }

        unsafe {
            let mut launch = ExecuteKernel::new(&kernel);
            launch
                .set_arg(&base_key_buffer)
                .set_arg(&target_hash_buffer)
                .set_arg(&(config.loop_count as cl_uint))
                .set_arg(&found_flag_buffer)
                .set_arg(&found_key_buffer)
                .set_global_work_size(work_items);
            if config.work_group_size > 0 && work_items % config.work_group_size == 0 {
                launch.set_local_work_size(config.work_group_size);
            }
            launch.enqueue_nd_range(&queue)?;
        }

        let mut found_flag = [0u32];
        unsafe {
            queue.enqueue_read_buffer(&found_flag_buffer, CL_BLOCKING, 0, &mut found_flag, &[])?;
        }

        total_candidates += config.batch_size as u64;
        window_candidates += config.batch_size as u64;

        if stats {
            maybe_report_stats(&mut last_report, &mut window_candidates, total_candidates);
        }

        if found_flag[0] != 0 {
            let mut found_key_words = [0u32; 8];
            unsafe {
                queue.enqueue_read_buffer(
                    &found_key_buffer,
                    CL_BLOCKING,
                    0,
                    &mut found_key_words,
                    &[],
                )?;
            }

            let private_key = le_words_to_private_key(found_key_words);

            if stats && window_candidates > 0 {
                flush_stats(last_report, window_candidates, total_candidates);
            }

            println!("Match found! Private key: {}", hex::encode(private_key));
            save_private_key_to_file(pubkey, private_key, "found_keys")
                .expect("Failed to save private key");
            return Ok(());
        }
    }
}

impl OpenClScanConfig {
    fn from_env() -> Result<Self, Box<dyn Error>> {
        let batch_size = parse_env_usize("BIT_SCAN_V3_BATCH_SIZE")
            .unwrap_or(DEFAULT_BATCH_SIZE)
            .max(1);
        let loop_count = parse_env_u32("BIT_SCAN_V3_ITEMS_PER_THREAD")
            .or_else(|| parse_env_u32("BIT_SCAN_V3_LOOP_COUNT"))
            .unwrap_or(DEFAULT_LOOP_COUNT)
            .max(1);
        let work_group_size = parse_env_usize("BIT_SCAN_V3_BLOCK_SIZE")
            .unwrap_or(DEFAULT_WORK_GROUP_SIZE)
            .max(1);
        let device_index = parse_env_usize("BIT_SCAN_V3_OPENCL_DEVICE_INDEX").unwrap_or(0);

        if batch_size > MAX_KERNEL_SPAN {
            return Err(format!(
                "BIT_SCAN_V3_BATCH_SIZE={} exceeds the OpenCL kernel span limit of {}",
                batch_size, MAX_KERNEL_SPAN
            )
            .into());
        }
        if batch_size % loop_count as usize != 0 {
            return Err(format!(
                "BIT_SCAN_V3_BATCH_SIZE ({batch_size}) must be divisible by BIT_SCAN_V3_ITEMS_PER_THREAD ({loop_count})"
            )
            .into());
        }

        Ok(Self {
            batch_size,
            loop_count,
            work_group_size,
            device_index,
        })
    }

    fn work_items(self) -> usize {
        self.batch_size / self.loop_count as usize
    }
}

fn ensure_opencl_runtime_path() {
    if env::var_os("OPENCL_DYLIB_PATH").is_some() {
        return;
    }

    for candidate in [
        PathBuf::from("/run/opengl-driver/lib/libOpenCL.so.1"),
        PathBuf::from("/usr/lib/libOpenCL.so.1"),
        PathBuf::from("/usr/local/lib/libOpenCL.so.1"),
    ] {
        if candidate.exists() {
            unsafe {
                env::set_var("OPENCL_DYLIB_PATH", candidate);
            }
            return;
        }
    }

    if let Some(candidate) = find_opencl_loader_in_nix_store() {
        unsafe {
            env::set_var("OPENCL_DYLIB_PATH", candidate);
        }
    }
}

fn sample_batch_base(bits: u32, batch_size: u32, rng: &mut impl Rng) -> u128 {
    let span = batch_size.saturating_sub(1) as u128;

    if bits == 128 {
        let mut base = rng.r#gen::<u128>() | (1u128 << 127);
        if base > u128::MAX - span {
            base = base.saturating_sub(span);
        }
        return base;
    }

    let min = 1u128 << (bits - 1);
    let max_exclusive = 1u128 << bits;
    let max_base = max_exclusive.saturating_sub(span + 1).max(min);
    rng.gen_range(min..=max_base)
}

fn u128_to_le_words(value: u128) -> [u32; 8] {
    let mut words = [0u32; 8];
    words[0] = value as u32;
    words[1] = (value >> 32) as u32;
    words[2] = (value >> 64) as u32;
    words[3] = (value >> 96) as u32;
    words
}

fn le_words_to_private_key(words: [u32; 8]) -> [u8; 32] {
    let mut private_key = [0u8; 32];

    for (idx, word) in words.iter().enumerate() {
        let start = 32 - ((idx + 1) * 4);
        private_key[start..start + 4].copy_from_slice(&word.to_be_bytes());
    }

    private_key
}

fn maybe_report_stats(
    last_report: &mut Instant,
    window_candidates: &mut u64,
    total_candidates: u64,
) {
    let elapsed = last_report.elapsed();
    if elapsed >= Duration::from_secs(1) {
        let secs = elapsed.as_secs_f64();
        if secs > 0.0 {
            let rate = *window_candidates as f64 / secs;
            println!(
                "Hashes: {:.2} per second (total processed {})",
                rate, total_candidates
            );
        }
        *window_candidates = 0;
        *last_report = Instant::now();
    }
}

fn flush_stats(last_report: Instant, window_candidates: u64, total_candidates: u64) {
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

fn parse_env_usize(key: &str) -> Option<usize> {
    env::var(key).ok()?.parse().ok()
}

fn parse_env_u32(key: &str) -> Option<u32> {
    env::var(key).ok()?.parse().ok()
}

fn load_opencl_source(vendor_root: &Path) -> Result<String, Box<dyn Error>> {
    let resources = [
        "inc_defines.h",
        "copyfromhashcat/inc_vendor.h",
        "copyfromhashcat/inc_types.h",
        "copyfromhashcat/inc_platform.h",
        "copyfromhashcat/inc_platform.cl",
        "copyfromhashcat/inc_common.h",
        "copyfromhashcat/inc_common.cl",
        "copyfromhashcat/inc_hash_sha256.h",
        "copyfromhashcat/inc_hash_sha256.cl",
        "copyfromhashcat/inc_hash_ripemd160.h",
        "copyfromhashcat/inc_hash_ripemd160.cl",
        "copyfromhashcat/inc_ecc_secp256k1.h",
        "copyfromhashcat/inc_ecc_secp256k1.cl",
        "inc_ecc_secp256k1custom.cl",
        "bit_scan_kernel.cl",
    ];

    let mut merged = String::new();

    for resource in resources {
        let content = fs::read_to_string(vendor_root.join(resource))?;
        for line in content.lines() {
            if line.trim_start().starts_with("#include") {
                continue;
            }
            merged.push_str(line);
            merged.push('\n');
        }
    }

    Ok(merged)
}

fn find_opencl_loader_in_nix_store() -> Option<PathBuf> {
    let store = PathBuf::from("/nix/store");
    let entries = fs::read_dir(&store).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy();

        if !(name.contains("ocl-icd") || name.contains("opencl-icd-loader")) {
            continue;
        }

        let candidate = path.join("lib/libOpenCL.so.1");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}
