//! Structured implementation of the second-generation pattern scan.
//!
//! The submodules expose isolated building blocks: high-level orchestration (`scan`),
//! the probabilistic generator (`generator`), and the analytics-backed metadata loader
//! (`model`). Shared math helpers and constants live in dedicated modules to keep
//! responsibilities narrow and testable.

pub mod scan;

mod constants;
mod generator;
mod model;
mod probability;

pub use scan::scan;

#[cfg(test)]
mod tests {
    use super::generator::PatternGenerator;
    use rand::{SeedableRng, rngs::StdRng};
    use std::collections::HashSet;

    fn bits_from_u128(value: u128, length: usize) -> Vec<u8> {
        (0..length)
            .rev()
            .map(|idx| ((value >> idx) & 1) as u8)
            .collect()
    }

    fn extract_runs(bits: &[u8]) -> Vec<(u8, usize)> {
        if bits.is_empty() {
            return Vec::new();
        }
        let mut runs = Vec::new();
        let mut current = bits[0];
        let mut length = 1usize;
        for &bit in &bits[1..] {
            if bit == current {
                length += 1;
            } else {
                runs.push((current, length));
                current = bit;
                length = 1;
            }
        }
        runs.push((current, length));
        runs
    }

    #[test]
    fn generator_respects_bit_length() {
        let mut generator = PatternGenerator::new();
        let mut rng = StdRng::seed_from_u64(42);
        let value = generator.generate(&mut rng, 71);
        assert_eq!(71, 128 - value.leading_zeros());
    }

    #[test]
    fn generator_produces_alternating_runs() {
        let mut generator = PatternGenerator::new();
        let mut rng = StdRng::seed_from_u64(1234);
        let value = generator.generate(&mut rng, 71);
        let bits = bits_from_u128(value, 71);
        let runs = extract_runs(&bits);
        assert!(!runs.is_empty());
        assert_eq!(runs[0].0, 1);
        for window in runs.windows(2) {
            assert_ne!(window[0].0, window[1].0);
        }
        assert_eq!(bits.len(), 71);
        assert_eq!(
            bits.iter().filter(|&&b| b == 1).count() + bits.iter().filter(|&&b| b == 0).count(),
            71
        );
    }

    #[test]
    fn generator_avoids_duplicates_across_calls() {
        let mut generator = PatternGenerator::new();
        let mut rng = StdRng::seed_from_u64(2024);
        let mut seen = HashSet::new();
        for _ in 0..5 {
            let value = generator.generate(&mut rng, 71);
            assert!(seen.insert(value));
        }
    }
}
