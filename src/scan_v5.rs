use std::{
    num::NonZeroU128,
    time::{Duration, Instant},
};

use k256::{
    AffinePoint, EncodedPoint, ProjectivePoint, Scalar,
    elliptic_curve::{
        bigint::U256,
        ops::Reduce,
        sec1::{FromEncodedPoint, ToEncodedPoint},
    },
};
use num_bigint::BigUint;
use num_traits::{One, ToPrimitive, Zero};

use crate::utils::{extract_hash160_from_base58_address, hash160, save_private_key_to_file};

const MAX_ATTEMPTS: u64 = 32;
const DIRECT_SOLVE_LIMIT: u128 = 1_000_000;

#[derive(Clone, Debug)]
struct Progression {
    first: BigUint,
    last: BigUint,
    step: NonZeroU128,
    len: BigUint,
}

#[derive(Clone, Debug)]
struct JumpSet {
    distances: Vec<BigUint>,
    points: Vec<ProjectivePoint>,
}

#[derive(Clone, Copy, Debug, Default)]
struct WalkStats {
    jumps: u128,
    attempts: u64,
}

pub fn scan(
    address: &str,
    bits: u32,
    stats: bool,
    public_key_hex: &str,
    multiple_of: NonZeroU128,
) -> Result<(), String> {
    if !(1..=160).contains(&bits) {
        return Err("bits must be between 1 and 160 for scan_v5".to_string());
    }

    let public_key_bytes = parse_hex(public_key_hex)?;
    let public_key = parse_public_key(&public_key_bytes)?;
    validate_public_key_address(address, &public_key_bytes)?;

    let progression = Progression::from_bits(bits, multiple_of)?;
    let step_scalar = scalar_from_u128(multiple_of.get());
    let first_scalar = scalar_from_biguint(&progression.first)?;
    let base = ProjectivePoint::GENERATOR * step_scalar;
    let target = ProjectivePoint::from(public_key) - (ProjectivePoint::GENERATOR * first_scalar);

    if stats {
        println!(
            "scan_v5: kangaroo interval [{}..={}], step {}, progression candidates {}",
            progression.first,
            progression.last,
            progression.step.get(),
            progression.len
        );
    }

    let mut stats_state = WalkStats::default();
    let started = Instant::now();
    let j = if progression.len <= BigUint::from(DIRECT_SOLVE_LIMIT) {
        solve_direct(target, base, progression.len, stats, &mut stats_state)
    } else {
        solve_kangaroo(target, base, progression.len, stats, &mut stats_state)
    }
    .ok_or_else(|| {
        format!(
            "scan_v5: kangaroo did not find a key after {} attempts and {} jumps",
            stats_state.attempts, stats_state.jumps
        )
    })?;

    let private_value = progression.first + (BigUint::from(progression.step.get()) * j);

    verify_solution(&private_value, public_key, address)?;
    let private_key = private_key_from_biguint(&private_value)?;

    if stats {
        let secs = started.elapsed().as_secs_f64();
        if secs > 0.0 {
            println!(
                "Jumps: {:.2} per second (total processed {})",
                stats_state.jumps as f64 / secs,
                stats_state.jumps
            );
        }
    }

    println!("Match found! Private key: {}", hex::encode(private_key));
    save_private_key_to_file(address, private_key, "found_keys")
        .map_err(|err| format!("Failed to save private key: {err}"))?;
    Ok(())
}

fn solve_direct(
    target: ProjectivePoint,
    base: ProjectivePoint,
    len: BigUint,
    stats: bool,
    stats_state: &mut WalkStats,
) -> Option<BigUint> {
    let mut current = ProjectivePoint::IDENTITY;
    let mut last_report = Instant::now();
    let mut last_jumps = 0u128;
    let len = len.to_u128()?;

    for index in 0..len {
        if current == target {
            return Some(BigUint::from(index));
        }
        current += base;
        stats_state.jumps += 1;
        maybe_report_stats(stats, &mut last_report, &mut last_jumps, stats_state.jumps);
    }

    None
}

fn solve_kangaroo(
    target: ProjectivePoint,
    base: ProjectivePoint,
    len: BigUint,
    stats: bool,
    stats_state: &mut WalkStats,
) -> Option<BigUint> {
    let mut last_report = Instant::now();
    let mut last_jumps = 0u128;

    for attempt in 0..MAX_ATTEMPTS {
        stats_state.attempts = attempt + 1;
        let jumps = JumpSet::new(base, &len, attempt);
        let tame_steps = ceil_sqrt(&len).to_u128()?.saturating_mul(2).max(8);
        let mut tame_distance = BigUint::zero();
        let mut tame = base * scalar_from_biguint(&(&len - BigUint::one())).ok()?;

        for _ in 0..tame_steps {
            let idx = jumps.index_for(tame, attempt);
            tame += jumps.points[idx];
            tame_distance += &jumps.distances[idx];
            stats_state.jumps += 1;
            maybe_report_stats(stats, &mut last_report, &mut last_jumps, stats_state.jumps);
        }

        let tame_endpoint = tame;
        let wild_limit = (&len - BigUint::one()) + &tame_distance;
        let mut wild = target;
        let mut wild_distance = BigUint::zero();

        while wild_distance <= wild_limit {
            if wild == tame_endpoint {
                let candidate = (&len - BigUint::one()) + &tame_distance - &wild_distance;
                if candidate < len {
                    return Some(candidate);
                }
                break;
            }

            let idx = jumps.index_for(wild, attempt);
            wild += jumps.points[idx];
            wild_distance += &jumps.distances[idx];
            stats_state.jumps += 1;
            maybe_report_stats(stats, &mut last_report, &mut last_jumps, stats_state.jumps);
        }
    }

    None
}

impl Progression {
    fn from_bits(bits: u32, step: NonZeroU128) -> Result<Self, String> {
        let min = BigUint::one() << (bits - 1);
        let max = (BigUint::one() << bits) - BigUint::one();
        let step_value = step.get();
        let step_big = BigUint::from(step_value);
        let remainder = &min % &step_big;
        let adjustment = if remainder.is_zero() {
            BigUint::zero()
        } else {
            &step_big - remainder
        };
        let first = min + adjustment;
        if first > max {
            return Err(format!(
                "no {bits}-bit private keys are divisible by {}",
                step_value
            ));
        }
        let len = ((&max - &first) / &step_big) + BigUint::one();
        let last = &first + ((&len - BigUint::one()) * &step_big);
        Ok(Self {
            first,
            last,
            step,
            len,
        })
    }
}

impl JumpSet {
    fn new(base: ProjectivePoint, len: &BigUint, attempt: u64) -> Self {
        let target_mean = ceil_sqrt(len).max(BigUint::from(2u32)) / 2u32;
        let mut count = 2usize;
        while count < 192 && (((BigUint::one() << count) - BigUint::one()) / count) <= target_mean {
            count += 1;
        }
        count = count.clamp(2, 192);

        let distances: Vec<BigUint> = (0..count)
            .map(|idx| {
                let salt = splitmix64((attempt << 32) ^ idx as u64) as u128;
                (BigUint::one() << idx) + BigUint::from(salt % count as u128)
            })
            .collect();
        let points = distances
            .iter()
            .map(|distance| base * scalar_from_biguint(distance).expect("jump scalar is valid"))
            .collect();

        Self { distances, points }
    }

    fn index_for(&self, point: ProjectivePoint, attempt: u64) -> usize {
        let affine = AffinePoint::from(point);
        let encoded = affine.to_encoded_point(true);
        let bytes = encoded.as_bytes();
        let mut lane = [0u8; 8];
        lane.copy_from_slice(&bytes[bytes.len() - 8..]);
        let mixed = u64::from_be_bytes(lane) ^ splitmix64(attempt);
        mixed as usize % self.distances.len()
    }
}

fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim().strip_prefix("0x").unwrap_or(input.trim());
    hex::decode(trimmed).map_err(|err| format!("invalid --public-key hex: {err}"))
}

fn parse_public_key(bytes: &[u8]) -> Result<AffinePoint, String> {
    let encoded = EncodedPoint::from_bytes(bytes)
        .map_err(|err| format!("invalid SEC public key encoding: {err}"))?;
    Option::<AffinePoint>::from(AffinePoint::from_encoded_point(&encoded))
        .ok_or_else(|| "SEC public key is not a valid secp256k1 point".to_string())
}

fn validate_public_key_address(address: &str, public_key_bytes: &[u8]) -> Result<(), String> {
    let target_hash = extract_hash160_from_base58_address(address);
    let supplied_hash = hash160(public_key_bytes);
    if supplied_hash != target_hash {
        return Err(
            "--public-key does not hash to the target P2PKH address; use the exact compressed or uncompressed SEC key for that address"
                .to_string(),
        );
    }
    Ok(())
}

fn verify_solution(
    private_value: &BigUint,
    public_key: AffinePoint,
    address: &str,
) -> Result<(), String> {
    let candidate = ProjectivePoint::GENERATOR * scalar_from_biguint(private_value)?;
    if AffinePoint::from(candidate) != public_key {
        return Err("kangaroo collision failed final EC verification".to_string());
    }

    let compressed = AffinePoint::from(candidate).to_encoded_point(true);
    let uncompressed = AffinePoint::from(candidate).to_encoded_point(false);
    let target_hash = extract_hash160_from_base58_address(address);
    if hash160(compressed.as_bytes()) != target_hash
        && hash160(uncompressed.as_bytes()) != target_hash
    {
        return Err("kangaroo result does not match target address hash".to_string());
    }
    Ok(())
}

fn scalar_from_u128(value: u128) -> Scalar {
    let mut bytes = [0u8; 32];
    bytes[16..].copy_from_slice(&value.to_be_bytes());
    <Scalar as Reduce<U256>>::reduce_bytes((&bytes).into())
}

fn scalar_from_biguint(value: &BigUint) -> Result<Scalar, String> {
    let bytes_vec = value.to_bytes_be();
    if bytes_vec.len() > 32 {
        return Err("scalar is wider than 256 bits".to_string());
    }
    let mut bytes = [0u8; 32];
    bytes[32 - bytes_vec.len()..].copy_from_slice(&bytes_vec);
    Ok(<Scalar as Reduce<U256>>::reduce_bytes((&bytes).into()))
}

fn private_key_from_biguint(value: &BigUint) -> Result<[u8; 32], String> {
    let bytes_vec = value.to_bytes_be();
    if bytes_vec.len() > 32 {
        return Err("private key is wider than 256 bits".to_string());
    }
    let mut bytes = [0u8; 32];
    bytes[32 - bytes_vec.len()..].copy_from_slice(&bytes_vec);
    Ok(bytes)
}

fn ceil_sqrt(value: &BigUint) -> BigUint {
    if *value <= BigUint::one() {
        return value.clone();
    }

    let mut low = BigUint::one();
    let mut high = BigUint::one() << value.bits().div_ceil(2);
    if &high * &high < *value {
        high <<= 1usize;
    }

    while low < high {
        let mid = (&low + &high) >> 1usize;
        if &mid * &mid >= *value {
            high = mid;
        } else {
            low = mid + BigUint::one();
        }
    }

    low
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn maybe_report_stats(
    enabled: bool,
    last_report: &mut Instant,
    last_jumps: &mut u128,
    total_jumps: u128,
) {
    if !enabled || last_report.elapsed() < Duration::from_secs(1) {
        return;
    }

    let elapsed = last_report.elapsed().as_secs_f64();
    let delta = total_jumps.saturating_sub(*last_jumps);
    if elapsed > 0.0 {
        println!(
            "Jumps: {:.2} per second (total processed {})",
            delta as f64 / elapsed,
            total_jumps
        );
    }
    *last_jumps = total_jumps;
    *last_report = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::{Progression, ceil_sqrt, solve_kangaroo};
    use k256::{ProjectivePoint, Scalar, elliptic_curve::PrimeField};
    use num_bigint::BigUint;
    use num_traits::One;
    use std::num::NonZeroU128;

    fn point_for_private(value: u128) -> ProjectivePoint {
        let mut key = [0u8; 32];
        key[16..].copy_from_slice(&value.to_be_bytes());
        let scalar = Scalar::from_repr(key.into()).unwrap();
        ProjectivePoint::GENERATOR * scalar
    }

    #[test]
    fn progression_keeps_only_requested_multiples() {
        let progression = Progression::from_bits(8, NonZeroU128::new(5).unwrap()).unwrap();
        assert_eq!(progression.first, BigUint::from(130u32));
        assert_eq!(progression.last, BigUint::from(255u32));
        assert_eq!(progression.len, BigUint::from(26u32));
    }

    #[test]
    fn ceil_sqrt_rounds_up() {
        assert_eq!(ceil_sqrt(&BigUint::from(1u32)), BigUint::from(1u32));
        assert_eq!(ceil_sqrt(&BigUint::from(2u32)), BigUint::from(2u32));
        assert_eq!(ceil_sqrt(&BigUint::from(15u32)), BigUint::from(4u32));
        assert_eq!(ceil_sqrt(&BigUint::from(16u32)), BigUint::from(4u32));
        assert_eq!(ceil_sqrt(&BigUint::from(17u32)), BigUint::from(5u32));
        assert_eq!(
            ceil_sqrt(&(BigUint::one() << 140usize)),
            BigUint::one() << 70usize
        );
    }

    #[test]
    fn kangaroo_solves_small_progression_index() {
        let first = 1u128 << 15;
        let step = 5u128;
        let secret_index = 1234u128;
        let secret = first + (step * secret_index);
        let base = ProjectivePoint::GENERATOR * Scalar::from(step as u64);
        let target =
            point_for_private(secret) - (ProjectivePoint::GENERATOR * Scalar::from(first as u64));
        let mut stats = super::WalkStats::default();

        assert_eq!(
            solve_kangaroo(target, base, BigUint::from(10_000u32), false, &mut stats),
            Some(BigUint::from(secret_index))
        );
    }
}
