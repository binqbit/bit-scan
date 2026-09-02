use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use crate::utils::{
    extract_hash160_from_base58_address, hash160, number_to_private_key,
    private_to_compressed_pubkey, random_number_with_bit_length, save_private_key_to_file,
};

const CANDIDATES_PER_THREAD: usize = 1_024;

pub fn scan(pubkey: &str, bits: u32, stats: bool, threads: usize) {
    assert!(threads > 0, "threads must be non-zero");
    assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

    let pubkey_hash = extract_hash160_from_base58_address(pubkey);
    let worker_pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|idx| format!("scan-v4-worker-{idx}"))
        .build()
        .expect("failed to build scan_v4 worker pool");
    let candidate_count = cpu_batch_size(bits, threads);
    let mut rng = rand::thread_rng();

    let found = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));

    let stats_handle = if stats {
        let found = Arc::clone(&found);
        let total = Arc::clone(&total);
        Some(thread::spawn(move || {
            let mut last = 0u64;
            loop {
                if found.load(Ordering::Relaxed) {
                    let current = total.load(Ordering::Relaxed);
                    let delta = current.saturating_sub(last);
                    if delta > 0 {
                        println!(
                            "Hashes: {:.2} per second (total processed {})",
                            delta as f64, current
                        );
                    } else {
                        println!("Hashes: total processed {}", current);
                    }
                    break;
                }
                thread::sleep(Duration::from_secs(1));
                let current = total.load(Ordering::Relaxed);
                let delta = current.saturating_sub(last);
                println!(
                    "Hashes: {:.2} per second (total processed {})",
                    delta as f64, current
                );
                last = current;
            }
        }))
    } else {
        None
    };

    loop {
        let base = random_number_with_bit_length(&mut rng, bits);
        let result = scan_batch(
            &worker_pool,
            &pubkey_hash,
            base,
            bits,
            candidate_count,
            total.as_ref(),
        );

        if let Some(private_key) = result {
            found.store(true, Ordering::Relaxed);
            println!("Match found! Private key: {}", hex::encode(private_key));
            save_private_key_to_file(pubkey, private_key, "found_keys")
                .expect("Failed to save private key");
            break;
        }
    }

    if let Some(handle) = stats_handle {
        let _ = handle.join();
    }
}

fn cpu_batch_size(bits: u32, threads: usize) -> usize {
    let keyspace_size = 1u128 << (bits - 1);
    let desired = threads.saturating_mul(CANDIDATES_PER_THREAD) as u128;
    desired.min(keyspace_size) as usize
}

fn candidate_for_offset(base: u128, bits: u32, offset: usize) -> u128 {
    let required_high_bit = 1u128 << (bits - 1);
    let lower_bits_mask = required_high_bit - 1;
    let lower_bits = (base & lower_bits_mask).wrapping_add(offset as u128) & lower_bits_mask;
    required_high_bit | lower_bits
}

fn scan_batch(
    worker_pool: &ThreadPool,
    pubkey_hash: &[u8; 20],
    base: u128,
    bits: u32,
    candidate_count: usize,
    total: &AtomicU64,
) -> Option<[u8; 32]> {
    worker_pool.install(|| {
        (0..candidate_count).into_par_iter().find_map_any(|offset| {
            let num = candidate_for_offset(base, bits, offset);
            total.fetch_add(1, Ordering::Relaxed);

            let private_key = number_to_private_key(num);
            let public_key = private_to_compressed_pubkey(&private_key);
            let derived_pubkey = hash160(&public_key);

            (derived_pubkey == *pubkey_hash).then_some(private_key)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::atomic::{AtomicU64, Ordering},
    };

    use rand::{SeedableRng, rngs::StdRng};
    use rayon::ThreadPoolBuilder;

    use super::{CANDIDATES_PER_THREAD, candidate_for_offset, cpu_batch_size, scan_batch};
    use crate::utils::{
        hash160, number_to_private_key, private_to_compressed_pubkey, random_number_with_bit_length,
    };

    const WIDTHS: [u32; 14] = [1, 7, 8, 9, 31, 32, 63, 64, 65, 70, 71, 72, 127, 128];

    #[test]
    fn cpu_batch_size_is_capped_to_the_requested_keyspace() {
        assert_eq!(cpu_batch_size(1, 8), 1);
        assert_eq!(cpu_batch_size(2, 1), 2);
        assert_eq!(cpu_batch_size(8, 8), 128);
        assert_eq!(cpu_batch_size(20, 1), CANDIDATES_PER_THREAD);
        assert_eq!(cpu_batch_size(20, 3), 3 * CANDIDATES_PER_THREAD);
        assert_eq!(cpu_batch_size(128, 8), 8 * CANDIDATES_PER_THREAD);
        assert_eq!(cpu_batch_size(128, usize::MAX), usize::MAX);
    }

    #[test]
    fn cpu_batches_are_unique_and_stay_inside_the_exact_bit_interval() {
        let mut rng = StdRng::seed_from_u64(0x004b_a7c4);

        for bits in WIDTHS {
            let candidate_count = cpu_batch_size(bits, 8);
            let min = 1u128 << (bits - 1);
            let max = if bits == 128 {
                u128::MAX
            } else {
                (1u128 << bits) - 1
            };

            for _ in 0..8 {
                let base = random_number_with_bit_length(&mut rng, bits);
                let candidates: HashSet<u128> = (0..candidate_count)
                    .map(|offset| candidate_for_offset(base, bits, offset))
                    .collect();

                assert_eq!(candidates.len(), candidate_count);
                assert!(candidates.iter().all(|candidate| {
                    *candidate >= min
                        && *candidate <= max
                        && u128::BITS - candidate.leading_zeros() == bits
                }));
            }
        }
    }

    #[test]
    fn cpu_batch_wraps_inside_the_requested_bit_interval() {
        assert_eq!(candidate_for_offset(254, 8, 0), 254);
        assert_eq!(candidate_for_offset(254, 8, 1), 255);
        assert_eq!(candidate_for_offset(254, 8, 2), 128);
        assert_eq!(candidate_for_offset(254, 8, 3), 129);
    }

    #[test]
    fn circular_batches_cover_every_key_with_equal_frequency() {
        let bits = 4;
        let min = 1u128 << (bits - 1);
        let keyspace_size = min as usize;
        let candidate_count = 3;
        let mut coverage = vec![0usize; keyspace_size];

        for base in min..(min * 2) {
            for offset in 0..candidate_count {
                let candidate = candidate_for_offset(base, bits, offset);
                coverage[(candidate - min) as usize] += 1;
            }
        }

        assert!(coverage.iter().all(|count| *count == candidate_count));
    }

    #[test]
    fn scan_batch_finds_a_candidate_across_the_low_u64_boundary() {
        let base = 0x8123_4567_89ab_cdef_ffff_ffff_ffff_fffeu128;
        let expected_number = 0x8123_4567_89ab_cdf0_0000_0000_0000_0000;
        let expected_key = number_to_private_key(expected_number);
        let target_hash = hash160(&private_to_compressed_pubkey(&expected_key));
        let worker_pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        let total = AtomicU64::new(0);

        let found = scan_batch(&worker_pool, &target_hash, base, 128, 4, &total);

        assert_eq!(candidate_for_offset(base, 128, 2), expected_number);
        assert_eq!(found, Some(expected_key));
        assert!((1..=4).contains(&total.load(Ordering::Relaxed)));
    }
}
