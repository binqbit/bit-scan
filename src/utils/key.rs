use k256::{EncodedPoint, ecdsa::SigningKey};

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

pub fn number_to_private_key(num: u128) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[16..32].copy_from_slice(&num.to_be_bytes());
    bytes
}

pub fn private_key_to_hex(private_key: [u8; 32]) -> String {
    hex::encode(private_key)
}
