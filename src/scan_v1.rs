use rand::Rng;

use crate::utils::{
    extract_hash160_from_base58_address, hash160, number_to_private_key,
    private_to_compressed_pubkey, save_private_key_to_file,
};

pub fn scan(pubkey: &str, bits: u32, view: bool) {
    let pubkey_hash = extract_hash160_from_base58_address(pubkey);

    let mut rng = rand::thread_rng();
    let min = 2u128.pow(bits - 1);
    let max = 2u128.pow(bits);

    loop {
        let num: u128 = rng.gen_range(min..=max);

        let private_key = number_to_private_key(num);
        let public_key = private_to_compressed_pubkey(&private_key);
        let derived_pubkey = hash160(&public_key);

        if view {
            println!("Private key: {:x}", num);
        }

        if derived_pubkey == pubkey_hash {
            println!("Match found! Private key: {}", hex::encode(private_key));
            save_private_key_to_file(pubkey, private_key, "found_keys")
                .expect("Failed to save private key");
            break;
        }
    }

    if view {
        println!("Press Enter to continue...");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Failed to read line");
    }
}
