use std::fmt;

use crate::utils::{
    extract_hash160_from_base58_address, hash160, private_to_compressed_pubkey,
    private_to_uncompressed_pubkey,
};

#[derive(Debug)]
pub enum CheckError {
    InvalidHex(hex::FromHexError),
    InvalidLength(usize),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::InvalidHex(err) => {
                write!(f, "private key must be valid hex: {err}")
            }
            CheckError::InvalidLength(len) => write!(
                f,
                "private key must not exceed 32 bytes (64 hex chars); got {len} bytes. \
                 Shorter values are left-padded with zeros."
            ),
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::InvalidHex(err) => Some(err),
            CheckError::InvalidLength(_) => None,
        }
    }
}

pub fn check(address: &str, private_key_input: &str) -> Result<bool, CheckError> {
    let trimmed = private_key_input.trim();
    let without_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    let bytes = hex::decode(without_prefix).map_err(CheckError::InvalidHex)?;
    if bytes.len() > 32 {
        return Err(CheckError::InvalidLength(bytes.len()));
    }

    let mut private_key = [0u8; 32];
    let start = 32 - bytes.len();
    private_key[start..].copy_from_slice(&bytes);

    let target_hash = extract_hash160_from_base58_address(address);
    let compressed_pubkey = private_to_compressed_pubkey(&private_key);
    let compressed_match = hash160(&compressed_pubkey) == target_hash;

    if compressed_match {
        println!("Private key matches {address} using compressed public key format.");
        return Ok(true);
    }

    let uncompressed_pubkey = private_to_uncompressed_pubkey(&private_key);
    let uncompressed_match = hash160(&uncompressed_pubkey) == target_hash;

    if uncompressed_match {
        println!("Private key matches {address} using uncompressed public key format.");
        return Ok(true);
    }

    println!("Private key does not match {address}.");
    Ok(false)
}
