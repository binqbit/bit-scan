use k256::{EncodedPoint, ecdsa::SigningKey};

pub fn private_to_compressed_pubkey(private_key: &[u8; 32]) -> [u8; 33] {
    let signing_key = SigningKey::from_bytes(private_key.into()).expect("invalid private key");
    let verify_key = signing_key.verifying_key();
    let pubkey_point = EncodedPoint::from(verify_key);
    let pubkey_bytes = pubkey_point.to_bytes();
    let mut out = [0u8; 33];
    out.copy_from_slice(&pubkey_bytes);
    out
}

pub fn private_to_uncompressed_pubkey(private_key: &[u8; 32]) -> [u8; 65] {
    let signing_key = SigningKey::from_bytes(private_key.into()).expect("invalid private key");
    let verify_key = signing_key.verifying_key();
    let pubkey_point = verify_key.to_encoded_point(false);
    let pubkey_bytes = pubkey_point.to_bytes();
    let mut out = [0u8; 65];
    out.copy_from_slice(&pubkey_bytes);
    out
}

pub fn number_to_private_key(num: u128) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[16..32].copy_from_slice(&num.to_be_bytes());
    bytes
}

pub fn private_key_to_hex(private_key: [u8; 32]) -> String {
    hex::encode(private_key)
}
