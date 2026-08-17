use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    num::NonZeroU128,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};

use k256::{AffinePoint, ProjectivePoint, elliptic_curve::sec1::ToEncodedPoint};
use num_bigint::BigUint;
use num_traits::{One, Zero};
use opencl3::{
    command_queue::{CL_QUEUE_PROFILING_ENABLE, CommandQueue},
    context::Context,
    device::{CL_DEVICE_TYPE_GPU, Device, get_all_devices},
    kernel::{ExecuteKernel, Kernel},
    memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE},
    program::Program,
    types::{CL_BLOCKING, cl_uint},
};

use crate::scan_v5::{
    JumpSet, KangarooProblem, ceil_sqrt, finish_solution, prepare_problem, scalar_from_biguint,
    splitmix64,
};

const KERNEL_NAME: &str = "bit_scan_kangaroo_step_kernel";
const COORD_WORDS: usize = 8;
const POINT_WORDS: usize = COORD_WORDS * 2;
const DIST_WORDS: usize = 8;
const DEFAULT_BATCH_STEPS: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Herd {
    Tame,
    Wild,
}

#[derive(Clone, Debug)]
struct DistinguishedPoint {
    herd: Herd,
    distance: BigUint,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct PointKey([u32; POINT_WORDS]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpenClKangarooConfig {
    compute_units: usize,
    work_items: usize,
    local_work_size: usize,
    batch_steps: u32,
}

pub fn scan(
    address: &str,
    bits: u32,
    stats: bool,
    public_key_hex: &str,
    multiple_of: NonZeroU128,
) -> Result<(), String> {
    let problem = prepare_problem(address, bits, public_key_hex, multiple_of)?;
    if problem.target == ProjectivePoint::IDENTITY {
        return finish_solution(&problem, BigUint::zero());
    }

    ensure_opencl_runtime_path();
    let device = select_opencl_device().map_err(|err| err.to_string())?;
    let context = Context::from_device(&device).map_err(|err| err.to_string())?;
    let queue =
        CommandQueue::create_default_with_properties(&context, CL_QUEUE_PROFILING_ENABLE, 0)
            .map_err(|err| err.to_string())?;

    let vendor_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/opencl");
    let kernel_source = load_opencl_source(&vendor_root).map_err(|err| err.to_string())?;
    let program = Program::create_and_build_from_source(&context, &kernel_source, "-cl-std=CL1.2")
        .map_err(|err| format!("OpenCL program build failed: {err}"))?;
    let kernel = Kernel::create(&program, KERNEL_NAME).map_err(|err| err.to_string())?;
    let config = OpenClKangarooConfig::from_device(&device, &kernel)?;
    let dp_bits = dp_bits_for_len(&problem.progression.len);
    let jump_set = JumpSet::new(problem.base, &problem.progression.len, 0);

    if stats {
        let device_name = device
            .name()
            .unwrap_or_else(|_| "unknown OpenCL GPU".to_string());
        println!(
            "scan_v6: OpenCL kangaroo on {device_name}; compute units {}, walkers {}, local size {}, batch steps {}, dp bits {}, jump points {}",
            config.compute_units,
            config.work_items,
            config.local_work_size,
            config.batch_steps,
            dp_bits,
            jump_set.distances.len()
        );
        println!(
            "scan_v6: kangaroo interval [{}..={}], step {}, progression candidates {}",
            problem.progression.first,
            problem.progression.last,
            problem.progression.step.get(),
            problem.progression.len
        );
    }

    run_opencl_walk(
        &problem, &jump_set, config, dp_bits, &context, &queue, &kernel, stats,
    )
}

fn run_opencl_walk(
    problem: &KangarooProblem,
    jump_set: &JumpSet,
    config: OpenClKangarooConfig,
    dp_bits: u32,
    context: &Context,
    queue: &CommandQueue,
    kernel: &Kernel,
    stats: bool,
) -> Result<(), String> {
    let mut state_x = vec![0u32; config.work_items * COORD_WORDS];
    let mut state_y = vec![0u32; config.work_items * COORD_WORDS];
    let mut distances = vec![0u32; config.work_items * DIST_WORDS];
    let herds = initialize_walkers(
        problem,
        config.work_items,
        &mut state_x,
        &mut state_y,
        &mut distances,
    )?;
    let jump_x = point_words_for_jump_set(jump_set, true)?;
    let jump_y = point_words_for_jump_set(jump_set, false)?;
    let jump_distances = distance_words_for_jump_set(jump_set)?;

    let mut state_x_buffer = unsafe {
        Buffer::<cl_uint>::create(context, CL_MEM_READ_WRITE, state_x.len(), ptr::null_mut())
            .map_err(|err| err.to_string())?
    };
    let mut state_y_buffer = unsafe {
        Buffer::<cl_uint>::create(context, CL_MEM_READ_WRITE, state_y.len(), ptr::null_mut())
            .map_err(|err| err.to_string())?
    };
    let mut distance_buffer = unsafe {
        Buffer::<cl_uint>::create(context, CL_MEM_READ_WRITE, distances.len(), ptr::null_mut())
            .map_err(|err| err.to_string())?
    };
    let mut jump_x_buffer = unsafe {
        Buffer::<cl_uint>::create(context, CL_MEM_READ_ONLY, jump_x.len(), ptr::null_mut())
            .map_err(|err| err.to_string())?
    };
    let mut jump_y_buffer = unsafe {
        Buffer::<cl_uint>::create(context, CL_MEM_READ_ONLY, jump_y.len(), ptr::null_mut())
            .map_err(|err| err.to_string())?
    };
    let mut jump_distance_buffer = unsafe {
        Buffer::<cl_uint>::create(
            context,
            CL_MEM_READ_ONLY,
            jump_distances.len(),
            ptr::null_mut(),
        )
        .map_err(|err| err.to_string())?
    };
    let hit_buffer = unsafe {
        Buffer::<cl_uint>::create(
            context,
            CL_MEM_READ_WRITE,
            config.work_items,
            ptr::null_mut(),
        )
        .map_err(|err| err.to_string())?
    };
    let steps_buffer = unsafe {
        Buffer::<cl_uint>::create(
            context,
            CL_MEM_READ_WRITE,
            config.work_items,
            ptr::null_mut(),
        )
        .map_err(|err| err.to_string())?
    };

    unsafe {
        queue
            .enqueue_write_buffer(&mut state_x_buffer, CL_BLOCKING, 0, &state_x, &[])
            .map_err(|err| err.to_string())?;
        queue
            .enqueue_write_buffer(&mut state_y_buffer, CL_BLOCKING, 0, &state_y, &[])
            .map_err(|err| err.to_string())?;
        queue
            .enqueue_write_buffer(&mut distance_buffer, CL_BLOCKING, 0, &distances, &[])
            .map_err(|err| err.to_string())?;
        queue
            .enqueue_write_buffer(&mut jump_x_buffer, CL_BLOCKING, 0, &jump_x, &[])
            .map_err(|err| err.to_string())?;
        queue
            .enqueue_write_buffer(&mut jump_y_buffer, CL_BLOCKING, 0, &jump_y, &[])
            .map_err(|err| err.to_string())?;
        queue
            .enqueue_write_buffer(
                &mut jump_distance_buffer,
                CL_BLOCKING,
                0,
                &jump_distances,
                &[],
            )
            .map_err(|err| err.to_string())?;
    }

    let mut hits = vec![0u32; config.work_items];
    let mut steps = vec![0u32; config.work_items];
    let mut table: HashMap<PointKey, DistinguishedPoint> = HashMap::new();
    let mut total_jumps = 0u128;
    let mut last_jumps = 0u128;
    let mut last_report = Instant::now();
    let started = Instant::now();

    loop {
        unsafe {
            let mut launch = ExecuteKernel::new(kernel);
            launch
                .set_arg(&state_x_buffer)
                .set_arg(&state_y_buffer)
                .set_arg(&distance_buffer)
                .set_arg(&jump_x_buffer)
                .set_arg(&jump_y_buffer)
                .set_arg(&jump_distance_buffer)
                .set_arg(&(jump_set.distances.len() as cl_uint))
                .set_arg(&(dp_bits as cl_uint))
                .set_arg(&(config.batch_steps as cl_uint))
                .set_arg(&hit_buffer)
                .set_arg(&steps_buffer)
                .set_global_work_size(config.work_items)
                .set_local_work_size(config.local_work_size)
                .enqueue_nd_range(queue)
                .map_err(|err| err.to_string())?
                .wait()
                .map_err(|err| err.to_string())?;

            queue
                .enqueue_read_buffer(&state_x_buffer, CL_BLOCKING, 0, &mut state_x, &[])
                .map_err(|err| err.to_string())?;
            queue
                .enqueue_read_buffer(&state_y_buffer, CL_BLOCKING, 0, &mut state_y, &[])
                .map_err(|err| err.to_string())?;
            queue
                .enqueue_read_buffer(&distance_buffer, CL_BLOCKING, 0, &mut distances, &[])
                .map_err(|err| err.to_string())?;
            queue
                .enqueue_read_buffer(&hit_buffer, CL_BLOCKING, 0, &mut hits, &[])
                .map_err(|err| err.to_string())?;
            queue
                .enqueue_read_buffer(&steps_buffer, CL_BLOCKING, 0, &mut steps, &[])
                .map_err(|err| err.to_string())?;
        }

        total_jumps += steps.iter().map(|&value| u128::from(value)).sum::<u128>();
        maybe_report_stats(
            stats,
            &mut last_report,
            &mut last_jumps,
            total_jumps,
            table.len(),
        );

        for (idx, &hit) in hits.iter().enumerate() {
            if hit == 0 {
                continue;
            }

            let key = point_key(&state_x, &state_y, idx);
            let distance = words_to_biguint(&distances[idx * DIST_WORDS..(idx + 1) * DIST_WORDS]);
            if let Some(existing) = table.get(&key) {
                if existing.herd != herds[idx] {
                    if let Some(candidate) = collision_candidate(
                        &problem.progression.len,
                        existing,
                        herds[idx],
                        &distance,
                    ) {
                        if candidate < problem.progression.len {
                            if stats {
                                let secs = started.elapsed().as_secs_f64();
                                if secs > 0.0 {
                                    println!(
                                        "Jumps: {:.2} per second (total processed {}, distinguished points {})",
                                        total_jumps as f64 / secs,
                                        total_jumps,
                                        table.len()
                                    );
                                }
                            }
                            return finish_solution(problem, candidate);
                        }
                    }
                }
            } else {
                table.insert(
                    key,
                    DistinguishedPoint {
                        herd: herds[idx],
                        distance,
                    },
                );
            }
        }
    }
}

fn initialize_walkers(
    problem: &KangarooProblem,
    work_items: usize,
    state_x: &mut [u32],
    state_y: &mut [u32],
    distances: &mut [u32],
) -> Result<Vec<Herd>, String> {
    let mut herds = Vec::with_capacity(work_items);
    let upper = &problem.progression.len - BigUint::one();
    let spacing =
        (ceil_sqrt(&problem.progression.len) / (work_items.max(1) as u32)).max(BigUint::one());

    for idx in 0..work_items {
        let herd = if idx % 2 == 0 { Herd::Tame } else { Herd::Wild };
        let lane = (idx / 2) + 1;
        let jitter = BigUint::from((splitmix64(idx as u64) & 0xffff) + 1);
        let offset = (&spacing * lane) + jitter;
        let offset_point = problem.base * scalar_from_biguint(&offset)?;
        let point = match herd {
            Herd::Tame => problem.base * scalar_from_biguint(&(&upper + &offset))?,
            Herd::Wild => problem.target + offset_point,
        };
        write_point_words(state_x, state_y, idx, point)?;
        write_biguint_words(distances, idx, &offset)?;
        herds.push(herd);
    }

    Ok(herds)
}

fn collision_candidate(
    len: &BigUint,
    existing: &DistinguishedPoint,
    new_herd: Herd,
    new_distance: &BigUint,
) -> Option<BigUint> {
    let upper = len - BigUint::one();
    let (tame_distance, wild_distance) = match (existing.herd, new_herd) {
        (Herd::Tame, Herd::Wild) => (&existing.distance, new_distance),
        (Herd::Wild, Herd::Tame) => (new_distance, &existing.distance),
        _ => return None,
    };
    let left = upper + tame_distance;
    (left >= *wild_distance).then(|| left - wild_distance)
}

fn point_words_for_jump_set(jump_set: &JumpSet, x_coordinate: bool) -> Result<Vec<u32>, String> {
    let mut words = Vec::with_capacity(jump_set.points.len() * COORD_WORDS);
    for point in &jump_set.points {
        let (x, y) = point_to_le_words(*point)?;
        words.extend_from_slice(if x_coordinate { &x } else { &y });
    }
    Ok(words)
}

fn distance_words_for_jump_set(jump_set: &JumpSet) -> Result<Vec<u32>, String> {
    let mut words = Vec::with_capacity(jump_set.distances.len() * DIST_WORDS);
    for distance in &jump_set.distances {
        words.extend_from_slice(&biguint_to_words(distance)?);
    }
    Ok(words)
}

fn write_point_words(
    state_x: &mut [u32],
    state_y: &mut [u32],
    idx: usize,
    point: ProjectivePoint,
) -> Result<(), String> {
    let (x, y) = point_to_le_words(point)?;
    let start = idx * COORD_WORDS;
    state_x[start..start + COORD_WORDS].copy_from_slice(&x);
    state_y[start..start + COORD_WORDS].copy_from_slice(&y);
    Ok(())
}

fn write_biguint_words(words: &mut [u32], idx: usize, value: &BigUint) -> Result<(), String> {
    let encoded = biguint_to_words(value)?;
    let start = idx * DIST_WORDS;
    words[start..start + DIST_WORDS].copy_from_slice(&encoded);
    Ok(())
}

fn point_key(state_x: &[u32], state_y: &[u32], idx: usize) -> PointKey {
    let mut key = [0u32; POINT_WORDS];
    let start = idx * COORD_WORDS;
    key[..COORD_WORDS].copy_from_slice(&state_x[start..start + COORD_WORDS]);
    key[COORD_WORDS..].copy_from_slice(&state_y[start..start + COORD_WORDS]);
    PointKey(key)
}

fn point_to_le_words(
    point: ProjectivePoint,
) -> Result<([u32; COORD_WORDS], [u32; COORD_WORDS]), String> {
    if point == ProjectivePoint::IDENTITY {
        return Err("OpenCL kangaroo cannot encode the point at infinity".to_string());
    }
    let affine = AffinePoint::from(point);
    let encoded = affine.to_encoded_point(false);
    let bytes = encoded.as_bytes();
    Ok((
        coord_to_le_words(&bytes[1..33]),
        coord_to_le_words(&bytes[33..65]),
    ))
}

fn coord_to_le_words(bytes: &[u8]) -> [u32; COORD_WORDS] {
    let mut words = [0u32; COORD_WORDS];
    for (idx, word) in words.iter_mut().enumerate() {
        let start = bytes.len() - ((idx + 1) * 4);
        *word = u32::from_be_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ]);
    }
    words
}

fn biguint_to_words(value: &BigUint) -> Result<[u32; DIST_WORDS], String> {
    let bytes = value.to_bytes_be();
    if bytes.len() > DIST_WORDS * 4 {
        return Err("kangaroo distance exceeded 256 bits".to_string());
    }
    let mut padded = [0u8; DIST_WORDS * 4];
    padded[DIST_WORDS * 4 - bytes.len()..].copy_from_slice(&bytes);
    Ok(coord_to_le_words(&padded))
}

fn words_to_biguint(words: &[u32]) -> BigUint {
    let mut bytes = [0u8; DIST_WORDS * 4];
    for (idx, word) in words.iter().enumerate() {
        let start = bytes.len() - ((idx + 1) * 4);
        bytes[start..start + 4].copy_from_slice(&word.to_be_bytes());
    }
    BigUint::from_bytes_be(&bytes)
}

fn dp_bits_for_len(len: &BigUint) -> u32 {
    ((len.bits() / 5) as u32).clamp(4, 20)
}

fn maybe_report_stats(
    enabled: bool,
    last_report: &mut Instant,
    last_jumps: &mut u128,
    total_jumps: u128,
    distinguished_points: usize,
) {
    if !enabled || last_report.elapsed() < Duration::from_secs(1) {
        return;
    }
    let elapsed = last_report.elapsed().as_secs_f64();
    let delta = total_jumps.saturating_sub(*last_jumps);
    if elapsed > 0.0 {
        println!(
            "Jumps: {:.2} per second (total processed {}, distinguished points {})",
            delta as f64 / elapsed,
            total_jumps,
            distinguished_points
        );
    }
    *last_jumps = total_jumps;
    *last_report = Instant::now();
}

impl OpenClKangarooConfig {
    fn from_device(device: &Device, kernel: &Kernel) -> Result<Self, String> {
        let compute_units =
            usize::try_from(device.max_compute_units().map_err(|err| err.to_string())?)
                .map_err(|err| err.to_string())?;
        if compute_units == 0 {
            return Err("OpenCL reported zero compute units".to_string());
        }

        let device_group = device
            .max_work_group_size()
            .map_err(|err| err.to_string())?;
        let kernel_group = kernel
            .get_work_group_size(device.id())
            .map_err(|err| err.to_string())?;
        let preferred = kernel
            .get_work_group_size_multiple(device.id())
            .map_err(|err| err.to_string())?
            .max(1);
        let local_work_size = preferred.min(device_group).min(kernel_group).max(1);
        let work_items = (compute_units * local_work_size * 2).max(local_work_size * 2);
        let work_items = if work_items % 2 == 0 {
            work_items
        } else {
            work_items + 1
        };

        Ok(Self {
            compute_units,
            work_items,
            local_work_size,
            batch_steps: DEFAULT_BATCH_STEPS,
        })
    }
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
        "bit_scan_kangaroo_kernel.cl",
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
    use super::{biguint_to_words, dp_bits_for_len, words_to_biguint};
    use num_bigint::BigUint;
    use num_traits::One;

    #[test]
    fn distance_words_round_trip_full_width_values() {
        let value = (BigUint::one() << 140usize) + BigUint::from(12345u32);
        let words = biguint_to_words(&value).unwrap();
        assert_eq!(words_to_biguint(&words), value);
    }

    #[test]
    fn dp_bits_are_bounded_for_small_and_large_ranges() {
        assert_eq!(dp_bits_for_len(&BigUint::from(256u32)), 4);
        assert_eq!(dp_bits_for_len(&(BigUint::one() << 140usize)), 20);
    }
}
