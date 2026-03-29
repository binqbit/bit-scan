//! Probability helpers used by both the generator and the analytics loader.
//!
//! The utilities here are small, deterministic building blocks that keep
//! the numerical plumbing separate from higher-level orchestration logic.

use rand::{Rng, distributions::Distribution};
use rand_distr::Gamma;

use super::constants::DIRICHLET_EPSILON;

/// Normalises a slice of weights in-place, falling back to a uniform
/// distribution when the sum cannot be trusted.
pub(crate) fn normalize(values: &mut [f64]) {
    let sum = values.iter().sum::<f64>();
    if sum <= 0.0 {
        let uniform = 1.0 / values.len().max(1) as f64;
        for value in values.iter_mut() {
            *value = uniform;
        }
    } else {
        for value in values.iter_mut() {
            *value /= sum;
        }
    }
}

/// Draws a Dirichlet sample by instantiating Gamma variates for each alpha.
///
/// The returned vector always forms a valid probability distribution thanks
/// to the uniform fallback when underflow or degenerate parameters occur.
pub(crate) fn dirichlet_sample<R: Rng + ?Sized>(alphas: &[f64], rng: &mut R) -> Vec<f64> {
    let mut draws = Vec::with_capacity(alphas.len());
    let mut sum = 0.0;
    for &alpha in alphas {
        let shape = alpha.max(DIRICHLET_EPSILON);
        let gamma = Gamma::new(shape, 1.0)
            .unwrap_or_else(|_| Gamma::new(DIRICHLET_EPSILON, 1.0).expect("invalid gamma"));
        let sample = gamma.sample(rng);
        draws.push(sample);
        sum += sample;
    }

    if sum <= 0.0 {
        let uniform = 1.0 / draws.len().max(1) as f64;
        for value in draws.iter_mut() {
            *value = uniform;
        }
    } else {
        for value in draws.iter_mut() {
            *value /= sum;
        }
    }

    draws
}
