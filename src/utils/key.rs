use k256::{EncodedPoint, ecdsa::SigningKey};
use rand::Rng;

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

/// Fits `value` into the exact `bits`-wide puzzle interval.
///
/// Bits above the requested width are cleared, the lower `bits - 1` bits are
/// preserved, and bit `bits - 1` is forced to one. The result therefore always
/// lies in `2^(bits - 1)..2^bits` without needing to construct the exclusive
/// upper bound (which is not representable for 128-bit candidates).
pub fn normalize_number_to_bit_length(value: u128, bits: u32) -> u128 {
    assert!((1..=128).contains(&bits), "bits must be between 1 and 128");

    let required_high_bit = 1u128 << (bits - 1);
    let lower_bits_mask = required_high_bit - 1;
    required_high_bit | (value & lower_bits_mask)
}

/// Samples a uniformly distributed number with exactly `bits` significant bits.
pub fn random_number_with_bit_length<R: Rng + ?Sized>(rng: &mut R, bits: u32) -> u128 {
    normalize_number_to_bit_length(rng.r#gen::<u128>(), bits)
}

pub fn private_key_to_hex(private_key: [u8; 32]) -> String {
    hex::encode(private_key)
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_number_to_bit_length, number_to_private_key, random_number_with_bit_length,
    };
    use rand::{SeedableRng, rngs::StdRng};

    const WIDTHS: [u32; 14] = [1, 7, 8, 9, 31, 32, 63, 64, 65, 70, 71, 72, 127, 128];

    fn bit_length(value: u128) -> u32 {
        u128::BITS - value.leading_zeros()
    }

    #[test]
    fn normalization_clears_unused_bits_and_forces_the_requested_high_bit() {
        let inputs = [0, 1, u128::MAX, 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210];

        for bits in WIDTHS {
            let required_high_bit = 1u128 << (bits - 1);
            let lower_bits_mask = required_high_bit - 1;

            for input in inputs {
                let normalized = normalize_number_to_bit_length(input, bits);
                assert_eq!(bit_length(normalized), bits);
                assert_eq!(normalized & required_high_bit, required_high_bit);
                assert_eq!(normalized & lower_bits_mask, input & lower_bits_mask);

                if bits < 128 {
                    assert_eq!(normalized >> bits, 0);
                }
            }
        }
    }

    #[test]
    fn random_sampler_always_returns_the_exact_requested_bit_length() {
        let mut rng = StdRng::seed_from_u64(0x5eed);

        for bits in WIDTHS {
            for _ in 0..64 {
                assert_eq!(
                    bit_length(random_number_with_bit_length(&mut rng, bits)),
                    bits
                );
            }
        }
    }

    #[test]
    fn partial_leading_byte_survives_private_key_serialization() {
        let lowest_71_bit_value = normalize_number_to_bit_length(0, 71);
        let highest_71_bit_value = normalize_number_to_bit_length(u128::MAX, 71);
        let low_key = number_to_private_key(lowest_71_bit_value);
        let high_key = number_to_private_key(highest_71_bit_value);

        assert!(low_key[..23].iter().all(|&byte| byte == 0));
        assert!(high_key[..23].iter().all(|&byte| byte == 0));
        assert_eq!(low_key[23], 0x40);
        assert_eq!(high_key[23], 0x7f);
        assert!(low_key[24..].iter().all(|&byte| byte == 0));
        assert!(high_key[24..].iter().all(|&byte| byte == 0xff));
    }
}
