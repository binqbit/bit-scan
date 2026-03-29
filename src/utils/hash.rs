use bs58;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

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
