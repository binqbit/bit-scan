use bs58;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

pub fn hash160(data: &[u8]) -> [u8; 20] {
    let sha256 = Sha256::digest(data);
    let ripemd = Ripemd160::digest(sha256);
    let mut out = [0u8; 20];
    out.copy_from_slice(&ripemd);
    out
}

pub fn extract_hash160_from_base58_address(addr: &str) -> [u8; 20] {
    let decoded = bs58::decode(addr).into_vec().expect("invalid base58");
    if decoded.len() != 25 || decoded[0] != 0x00 {
        panic!("Not a valid P2PKH address");
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&decoded[1..21]);
    out
}
