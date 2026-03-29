use std::time::{Duration, Instant};

use rand::Rng;

use crate::utils::{
    extract_hash160_from_base58_address, hash160, number_to_private_key,
    private_to_compressed_pubkey, save_private_key_to_file,
};

pub fn scan(pubkey: &str, bits: u32, stats: bool) {
    let pubkey_hash = extract_hash160_from_base58_address(pubkey);

    let mut rng = rand::thread_rng();
    let min = 2u128.pow(bits - 1);
    let max = 2u128.pow(bits);

    let mut total_candidates: u64 = 0;
    let mut window_candidates: u64 = 0;
    let mut last_report = Instant::now();

    loop {
        let num: u128 = rng.gen_range(min..=max);

        let private_key = number_to_private_key(num);
        let public_key = private_to_compressed_pubkey(&private_key);
        let derived_pubkey = hash160(&public_key);

        total_candidates += 1;
        window_candidates += 1;

        if stats && last_report.elapsed() >= Duration::from_secs(1) {
            let elapsed = last_report.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                let rate = window_candidates as f64 / elapsed;
                println!(
                    "Hashes: {:.2} per second (total processed {})",
                    rate, total_candidates
                );
            }
            window_candidates = 0;
            last_report = Instant::now();
        }

        if derived_pubkey == pubkey_hash {
            if stats && window_candidates > 0 {
                let elapsed = last_report.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let rate = window_candidates as f64 / elapsed;
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
            break;
        }
    }
}
