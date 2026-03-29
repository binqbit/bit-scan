use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::utils::{
    extract_hash160_from_base58_address, hash160, number_to_private_key,
    private_to_compressed_pubkey, save_private_key_to_file,
};

pub fn scan(pubkey: &str, bits: u32, stats: bool, threads: usize) {
    assert!(threads > 0, "threads must be non-zero");
    assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

    let pubkey_hash = Arc::new(extract_hash160_from_base58_address(pubkey));
    let target_address: Arc<str> = Arc::from(pubkey.to_owned());

    let min = 2u128.pow(bits - 1);
    let max = 2u128.pow(bits);

    let found = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));
    let mut seed_rng = rand::thread_rng();
    let seed_values: Vec<u64> = (0..threads).map(|_| seed_rng.r#gen::<u64>()).collect();
    let seeds = Arc::new(seed_values);

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

    let mut handles = Vec::with_capacity(threads);
    for thread_id in 0..threads {
        let pubkey_hash = Arc::clone(&pubkey_hash);
        let target_address = Arc::clone(&target_address);
        let found = Arc::clone(&found);
        let total = Arc::clone(&total);
        let seeds = Arc::clone(&seeds);

        handles.push(thread::spawn(move || {
            let seed = seeds[thread_id];
            let mut rng = StdRng::seed_from_u64(
                seed ^ (thread_id as u64)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(0xDEADBEEF),
            );
            while !found.load(Ordering::Relaxed) {
                let num = rng.gen_range(min..=max);
                total.fetch_add(1, Ordering::Relaxed);

                let private_key = number_to_private_key(num);
                let public_key = private_to_compressed_pubkey(&private_key);
                let derived_pubkey = hash160(&public_key);

                if derived_pubkey == *pubkey_hash {
                    if found
                        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                    {
                        println!("Match found! Private key: {}", hex::encode(private_key));
                        save_private_key_to_file(
                            target_address.as_ref(),
                            private_key,
                            "found_keys",
                        )
                        .expect("Failed to save private key");
                    }
                    break;
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    if let Some(handle) = stats_handle {
        let _ = handle.join();
    }
}
