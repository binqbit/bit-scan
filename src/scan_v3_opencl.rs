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
    event::Event,
    kernel::{ExecuteKernel, Kernel},
    memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE},
    program::Program,
    types::{CL_BLOCKING, cl_uint},
};

use crate::utils::{
    extract_hash160_from_base58_address, random_batch_base_with_bit_length,
    save_private_key_to_file,
};

const MAX_KERNEL_SPAN: usize = u32::MAX as usize;
const TARGET_KERNEL_TIME_NS: u64 = 250_000_000;
const KERNEL_NAME: &str = "bit_scan_match_kernel";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenClScanConfig {
    compute_units: usize,
    work_items: usize,
    local_work_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenClHardwareLimits {
    compute_units: usize,
    device_max_work_group_size: usize,
    device_max_work_item_size_x: usize,
    kernel_max_work_group_size: usize,
    preferred_work_group_multiple: usize,
    compile_work_group_size: [usize; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KernelTimeController {
    loop_count: u32,
}

pub fn scan(pubkey: &str, bits: u32, stats: bool) -> Result<(), Box<dyn Error>> {
    assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

    ensure_opencl_runtime_path();

    let target_hash = extract_hash160_from_base58_address(pubkey);

    let device = select_opencl_device()?;
    let context = Context::from_device(&device)?;
    let queue =
        CommandQueue::create_default_with_properties(&context, CL_QUEUE_PROFILING_ENABLE, 0)?;

    let vendor_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/opencl");
    let kernel_source = load_opencl_source(&vendor_root)?;
    let build_opts = "-cl-std=CL1.2";
    let program = Program::create_and_build_from_source(&context, &kernel_source, build_opts)
        .map_err(|err| format!("OpenCL program build failed: {err}"))?;
    let kernel = Kernel::create(&program, KERNEL_NAME)?;
    let config = OpenClScanConfig::from_hardware(query_hardware_limits(&device, &kernel)?)?;

    if stats {
        let device_name = device
            .name()
            .unwrap_or_else(|_| "unknown OpenCL GPU".to_string());
        println!(
            "scan_v3: OpenCL auto-selected {device_name}; compute units {}, work-items {}, local size {}, adaptive batch",
            config.compute_units, config.work_items, config.local_work_size
        );
    }

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
    let mut controller = KernelTimeController::new();
    let zero_flag = [0u32];

    loop {
        let launch_shape = config.launch_shape(bits, controller.loop_count);
        let base = random_batch_base_with_bit_length(&mut rng, bits, launch_shape.candidate_count);
        let base_words = u128_to_le_words(base);

        unsafe {
            queue.enqueue_write_buffer(&mut base_key_buffer, CL_BLOCKING, 0, &base_words, &[])?;
            queue.enqueue_write_buffer(&mut found_flag_buffer, CL_BLOCKING, 0, &zero_flag, &[])?;
        }

        let host_kernel_start = Instant::now();
        let kernel_event = unsafe {
            let mut launch = ExecuteKernel::new(&kernel);
            launch
                .set_arg(&base_key_buffer)
                .set_arg(&target_hash_buffer)
                .set_arg(&(launch_shape.loop_count as cl_uint))
                .set_arg(&found_flag_buffer)
                .set_arg(&found_key_buffer)
                .set_global_work_size(launch_shape.work_items);
            if let Some(local_work_size) = launch_shape.local_work_size {
                launch.set_local_work_size(local_work_size);
            }
            launch.enqueue_nd_range(&queue)?
        };
        kernel_event.wait()?;
        controller.observe(
            launch_shape.loop_count,
            kernel_elapsed_ns(&kernel_event, host_kernel_start.elapsed()),
        );

        let mut found_flag = [0u32];
        unsafe {
            queue.enqueue_read_buffer(&found_flag_buffer, CL_BLOCKING, 0, &mut found_flag, &[])?;
        }

        total_candidates += launch_shape.candidate_count as u64;
        window_candidates += launch_shape.candidate_count as u64;

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
    fn from_hardware(limits: OpenClHardwareLimits) -> Result<Self, String> {
        if limits.compute_units == 0 {
            return Err("OpenCL reported zero compute units".to_string());
        }
        if limits.device_max_work_group_size == 0
            || limits.device_max_work_item_size_x == 0
            || limits.kernel_max_work_group_size == 0
        {
            return Err("OpenCL reported a zero work-group limit".to_string());
        }
        if limits.preferred_work_group_multiple == 0 {
            return Err("OpenCL reported a zero preferred work-group multiple".to_string());
        }

        let hard_local_limit = limits
            .device_max_work_group_size
            .min(limits.device_max_work_item_size_x)
            .min(limits.kernel_max_work_group_size);
        let compiled = limits.compile_work_group_size;
        let local_work_size = if compiled[0] > 0 {
            if compiled[1] > 1 || compiled[2] > 1 {
                return Err(format!(
                    "OpenCL kernel requires a non-1D work-group of {compiled:?}"
                ));
            }
            if compiled[0] > hard_local_limit {
                return Err(format!(
                    "OpenCL kernel requires local size {}, above the hardware limit {hard_local_limit}",
                    compiled[0]
                ));
            }
            compiled[0]
        } else {
            let aligned =
                hard_local_limit - (hard_local_limit % limits.preferred_work_group_multiple);
            if aligned == 0 {
                hard_local_limit
            } else {
                aligned
            }
        };

        let target_work_items = limits
            .compute_units
            .checked_mul(limits.device_max_work_group_size)
            .ok_or_else(|| "OpenCL hardware work-item count overflowed usize".to_string())?;
        let max_aligned_work_items = (MAX_KERNEL_SPAN / local_work_size) * local_work_size;
        if max_aligned_work_items == 0 {
            return Err("OpenCL local size exceeds the kernel span".to_string());
        }
        let work_items = if target_work_items >= max_aligned_work_items {
            max_aligned_work_items
        } else {
            round_up_to_multiple(target_work_items, local_work_size)
                .ok_or_else(|| "OpenCL work-item alignment overflowed usize".to_string())?
        };

        Ok(Self {
            compute_units: limits.compute_units,
            work_items,
            local_work_size,
        })
    }

    fn launch_shape(self, bits: u32, requested_loop_count: u32) -> OpenClLaunchShape {
        let keyspace_size = 1u128 << (bits - 1);
        let work_items = (self.work_items as u128).min(keyspace_size) as usize;
        let candidate_limit = keyspace_size.min(MAX_KERNEL_SPAN as u128);
        let max_loop_count = (candidate_limit / work_items as u128)
            .min(u32::MAX as u128)
            .max(1) as u32;
        let loop_count = requested_loop_count.clamp(1, max_loop_count);
        let candidate_count = work_items * loop_count as usize;
        let local_work_size = work_items
            .is_multiple_of(self.local_work_size)
            .then_some(self.local_work_size);

        OpenClLaunchShape {
            candidate_count,
            loop_count,
            work_items,
            local_work_size,
        }
    }
}

impl KernelTimeController {
    const fn new() -> Self {
        Self { loop_count: 1 }
    }

    fn observe(&mut self, actual_loop_count: u32, elapsed_ns: u64) {
        if elapsed_ns == 0 {
            return;
        }

        let proportional = (u128::from(actual_loop_count) * u128::from(TARGET_KERNEL_TIME_NS))
            .div_ceil(u128::from(elapsed_ns))
            .clamp(1, u128::from(u32::MAX)) as u32;
        let lower = (actual_loop_count / 2).max(1);
        let upper = actual_loop_count.saturating_mul(2).max(1);
        self.loop_count = proportional.clamp(lower, upper);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenClLaunchShape {
    candidate_count: usize,
    loop_count: u32,
    work_items: usize,
    local_work_size: Option<usize>,
}

fn query_hardware_limits(
    device: &Device,
    kernel: &Kernel,
) -> Result<OpenClHardwareLimits, Box<dyn Error>> {
    let device_name = device
        .name()
        .unwrap_or_else(|_| "unknown OpenCL GPU".to_string());
    let work_item_sizes = device
        .max_work_item_sizes()
        .map_err(|err| format!("failed to query work-item limits for {device_name}: {err}"))?;
    let compile_work_group_size =
        kernel
            .get_compile_work_group_size(device.id())
            .map_err(|err| {
                format!("failed to query compiled work-group size for {device_name}: {err}")
            })?;

    let device_max_work_item_size_x = *work_item_sizes
        .first()
        .ok_or_else(|| format!("OpenCL returned no work-item limits for {device_name}"))?;
    if compile_work_group_size.len() < 3 {
        return Err(format!(
            "OpenCL returned an invalid compiled work-group shape for {device_name}"
        )
        .into());
    }

    Ok(OpenClHardwareLimits {
        compute_units: usize::try_from(device.max_compute_units()?)?,
        device_max_work_group_size: device.max_work_group_size()?,
        device_max_work_item_size_x,
        kernel_max_work_group_size: kernel.get_work_group_size(device.id())?,
        preferred_work_group_multiple: kernel.get_work_group_size_multiple(device.id())?,
        compile_work_group_size: [
            compile_work_group_size[0],
            compile_work_group_size[1],
            compile_work_group_size[2],
        ],
    })
}

fn select_opencl_device() -> Result<Device, Box<dyn Error>> {
    let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU)?;
    let mut best = None;

    for (ordinal, device_id) in device_ids.into_iter().enumerate() {
        let device = Device::new(device_id);
        if !device.available().unwrap_or(false) || !device.compiler_available().unwrap_or(false) {
            continue;
        }

        let Ok(compute_units) = device.max_compute_units() else {
            continue;
        };
        let clock_rate = device.max_clock_frequency().unwrap_or(1).max(1);
        let global_memory = device.global_mem_size().unwrap_or(0);
        let score = u64::from(compute_units) * u64::from(clock_rate);

        if best
            .as_ref()
            .is_none_or(|(best_score, best_memory, best_ordinal, _)| {
                (score, global_memory, usize::MAX - ordinal)
                    > (*best_score, *best_memory, usize::MAX - *best_ordinal)
            })
        {
            best = Some((score, global_memory, ordinal, device_id));
        }
    }

    best.map(|(_, _, _, device_id)| Device::new(device_id))
        .ok_or_else(|| "no available OpenCL GPU with a compiler was found".into())
}

fn round_up_to_multiple(value: usize, multiple: usize) -> Option<usize> {
    value
        .checked_add(multiple.checked_sub(1)?)
        .map(|rounded| (rounded / multiple) * multiple)
}

fn kernel_elapsed_ns(event: &Event, host_elapsed: Duration) -> u64 {
    event
        .profiling_command_start()
        .ok()
        .zip(event.profiling_command_end().ok())
        .and_then(|(start, end)| end.checked_sub(start))
        .filter(|elapsed| *elapsed > 0)
        .unwrap_or_else(|| u64::try_from(host_elapsed.as_nanos()).unwrap_or(u64::MAX))
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

#[cfg(test)]
mod tests {
    use super::{
        KernelTimeController, OpenClHardwareLimits, OpenClScanConfig, TARGET_KERNEL_TIME_NS,
        round_up_to_multiple,
    };
    use crate::utils::random_batch_base_with_bit_length;
    use rand::{SeedableRng, rngs::StdRng};

    const WIDTHS: [u32; 14] = [1, 7, 8, 9, 31, 32, 63, 64, 65, 70, 71, 72, 127, 128];

    fn test_limits() -> OpenClHardwareLimits {
        OpenClHardwareLimits {
            compute_units: 20,
            device_max_work_group_size: 1_024,
            device_max_work_item_size_x: 1_024,
            kernel_max_work_group_size: 256,
            preferred_work_group_multiple: 32,
            compile_work_group_size: [0, 0, 0],
        }
    }

    fn test_config() -> OpenClScanConfig {
        OpenClScanConfig::from_hardware(test_limits()).unwrap()
    }

    #[test]
    fn hardware_limits_determine_opencl_parallelism() {
        let config = test_config();

        assert_eq!(config.compute_units, 20);
        assert_eq!(config.local_work_size, 256);
        assert_eq!(config.work_items, 20 * 1_024);
    }

    #[test]
    fn local_size_respects_preferred_multiple_and_compiled_shape() {
        let rounded = OpenClScanConfig::from_hardware(OpenClHardwareLimits {
            kernel_max_work_group_size: 250,
            ..test_limits()
        })
        .unwrap();
        assert_eq!(rounded.local_work_size, 224);
        assert_eq!(rounded.work_items % rounded.local_work_size, 0);

        let compiled = OpenClScanConfig::from_hardware(OpenClHardwareLimits {
            compile_work_group_size: [128, 1, 1],
            ..test_limits()
        })
        .unwrap();
        assert_eq!(compiled.local_work_size, 128);

        assert!(
            OpenClScanConfig::from_hardware(OpenClHardwareLimits {
                compile_work_group_size: [128, 2, 1],
                ..test_limits()
            })
            .is_err()
        );
    }

    #[test]
    fn invalid_or_overflowing_hardware_limits_are_rejected() {
        assert!(
            OpenClScanConfig::from_hardware(OpenClHardwareLimits {
                compute_units: 0,
                ..test_limits()
            })
            .is_err()
        );
        assert!(
            OpenClScanConfig::from_hardware(OpenClHardwareLimits {
                preferred_work_group_multiple: 0,
                ..test_limits()
            })
            .is_err()
        );
        assert!(
            OpenClScanConfig::from_hardware(OpenClHardwareLimits {
                compute_units: usize::MAX,
                device_max_work_group_size: 2,
                ..test_limits()
            })
            .is_err()
        );
    }

    #[test]
    fn launch_shape_never_exceeds_the_requested_bit_keyspace() {
        let config = test_config();

        for bits in WIDTHS {
            let shape = config.launch_shape(bits, 8);
            let keyspace_size = 1u128 << (bits - 1);

            assert!(shape.candidate_count > 0);
            assert!(shape.candidate_count as u128 <= keyspace_size);
            assert_eq!(
                shape.work_items * shape.loop_count as usize,
                shape.candidate_count
            );
            if let Some(local_work_size) = shape.local_work_size {
                assert_eq!(shape.work_items % local_work_size, 0);
            }
        }
    }

    #[test]
    fn sampled_opencl_batch_stays_inside_the_exact_bit_interval() {
        let config = test_config();
        let mut rng = StdRng::seed_from_u64(0x0c1);

        for bits in WIDTHS {
            let shape = config.launch_shape(bits, 8);
            let min = 1u128 << (bits - 1);
            let max = if bits == 128 {
                u128::MAX
            } else {
                (1u128 << bits) - 1
            };

            for _ in 0..64 {
                let base = random_batch_base_with_bit_length(&mut rng, bits, shape.candidate_count);
                let last = base + shape.candidate_count as u128 - 1;
                assert!(base >= min);
                assert!(last <= max);
                assert_eq!(u128::BITS - base.leading_zeros(), bits);
                assert_eq!(u128::BITS - last.leading_zeros(), bits);
            }
        }
    }

    #[test]
    fn kernel_time_controller_converges_without_large_jumps() {
        let mut controller = KernelTimeController::new();

        controller.observe(1, TARGET_KERNEL_TIME_NS / 4);
        assert_eq!(controller.loop_count, 2);

        controller.observe(2, TARGET_KERNEL_TIME_NS * 4);
        assert_eq!(controller.loop_count, 1);

        controller.observe(1, 0);
        assert_eq!(controller.loop_count, 1);
    }

    #[test]
    fn alignment_helper_checks_overflow() {
        assert_eq!(round_up_to_multiple(20_480, 256), Some(20_480));
        assert_eq!(round_up_to_multiple(20_481, 256), Some(20_736));
        assert_eq!(round_up_to_multiple(1, 0), None);
        assert_eq!(round_up_to_multiple(usize::MAX, 2), None);
    }
}
