//! Top-level entry point that wires the generator into the CLI workflow.
//!
//! This module owns the looping search that feeds generated candidates into the
//! Bitcoin key utilities.

use rand::thread_rng;

use crate::utils::{
    extract_hash160_from_base58_address, hash160, number_to_private_key,
    private_to_compressed_pubkey, save_private_key_to_file,
};

use super::generator::PatternGenerator;

/// Attempts to discover the private key whose compressed pubkey matches `pubkey`.
pub fn scan(pubkey: &str, bits: u32, _stats: bool) {
    assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

    let pubkey_hash = extract_hash160_from_base58_address(pubkey);
    let mut generator = PatternGenerator::new();
    let mut rng = thread_rng();

    loop {
        let num = generator.generate(&mut rng, bits);
        let private_key = number_to_private_key(num);
        let public_key = private_to_compressed_pubkey(&private_key);
        let derived_pubkey = hash160(&public_key);

        if derived_pubkey == pubkey_hash {
            println!("Match found! Private key: {}", hex::encode(private_key));
            save_private_key_to_file(pubkey, private_key, "found_keys")
                .expect("Failed to save private key");
            break;
        }
    }
}
