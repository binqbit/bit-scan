//! Shared configuration values for the scan v2 pipeline.
//!
//! Centralising these constants keeps the tuning knobs in one place and
//! documents the assumptions baked into the generator and model loaders.

pub(crate) const ANALYTICS_DIR: &str = "analytics";
pub(crate) const TARGET_BASE_LENGTH: usize = 71;
pub(crate) const SHARE_ALPHA: f64 = 0.6;
pub(crate) const SHARE_BETA: f64 = 1.0;
pub(crate) const START_SMOOTHING: f64 = 0.5;
pub(crate) const TERMINAL_SMOOTHING: f64 = 0.5;
pub(crate) const TRANSITION_SMOOTHING: f64 = 0.5;
pub(crate) const DIRICHLET_EPSILON: f64 = 0.05;
pub(crate) const WEIGHT_EPSILON: f64 = 1e-6;
