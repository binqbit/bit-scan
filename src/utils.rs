use std::io::Write;

use bs58;
use k256::{EncodedPoint, ecdsa::SigningKey};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

pub fn private_to_compressed_pubkey(private_key: &[u8; 32]) -> Vec<u8> {
    let signing_key = SigningKey::from_bytes(private_key.into()).expect("invalid private key");
    let verify_key = signing_key.verifying_key();
    let pubkey_point = EncodedPoint::from(verify_key);
    pubkey_point.to_bytes().to_vec()
}

pub fn private_to_uncompressed_pubkey(private_key: &[u8; 32]) -> Vec<u8> {
    let signing_key = SigningKey::from_bytes(private_key.into()).expect("invalid private key");
    let verify_key = signing_key.verifying_key();
    let pubkey_point = verify_key.to_encoded_point(false);
    pubkey_point.to_bytes().to_vec()
}

pub fn hash160(data: &[u8]) -> Vec<u8> {
    let sha256 = Sha256::digest(data);
    Ripemd160::digest(sha256).to_vec()
}

pub fn extract_hash160_from_base58_address(addr: &str) -> Vec<u8> {
    let decoded = bs58::decode(addr).into_vec().expect("invalid base58");
    if decoded.len() != 25 || decoded[0] != 0x00 {
        panic!("Not a valid P2PKH address");
    }
    decoded[1..21].to_vec()
}

pub fn number_to_private_key(num: u128) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[16..32].copy_from_slice(&num.to_be_bytes());
    bytes
}

pub fn private_key_to_hex(private_key: [u8; 32]) -> String {
    hex::encode(private_key)
}

pub fn save_private_key_to_file(
    public_key: &str,
    private_key: [u8; 32],
    file_path: &str,
) -> std::io::Result<()> {
    if !std::path::Path::new(file_path).exists() {
        std::fs::create_dir_all(file_path)?;
    }

    let priv_hex = private_key_to_hex(private_key);
    let mut file = std::fs::File::create(format!("{}/{}.priv", file_path, public_key))?;
    file.write_all(&priv_hex.as_bytes())?;
    Ok(())
}
