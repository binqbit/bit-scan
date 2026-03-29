//! Probabilistic pattern generator used by scan v2.
//!
//! The generator encapsulates the DFS search with analytics-informed
//! heuristics. It maintains all intermediate state in fixed-size buffers
//! to minimise allocations, while the public API stays simple: feed it a
//! bit target and it produces candidate private keys.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use rand::{
    Rng,
    distributions::{Distribution, WeightedIndex},
};
use rand_distr::Normal;

use smallvec::SmallVec;

use super::{
    constants::{DIRICHLET_EPSILON, WEIGHT_EPSILON},
    model::Model,
    probability::dirichlet_sample,
};

/// Drives the alternating-run search using the analytics `Model` as guidance.
pub(crate) struct PatternGenerator {
    model: Model,
    params: GenerationParams,
    seen: HashSet<BitPatternKey>,
    candidates: Vec<Candidate>,
    evaluations: Vec<EvaluatedPattern>,
    histogram_best: HashMap<Vec<u8>, f64>,
    batch_keys: HashSet<BitPatternKey>,
    desired_counts: Vec<f64>,
    run_count_buf: Vec<usize>,
    weight_buffer: Vec<f64>,
    stats_counts: Vec<usize>,
    stats_lengths: Vec<usize>,
    histogram_key_buffer: Vec<u8>,
}

impl PatternGenerator {
    /// Builds a generator with the latest analytics snapshot.
    pub(crate) fn new() -> Self {
        let model = Model::load().expect("failed to load analytics model");
        PatternGenerator {
            model,
            params: GenerationParams::default(),
            seen: HashSet::new(),
            candidates: Vec::new(),
            evaluations: Vec::new(),
            histogram_best: HashMap::new(),
            batch_keys: HashSet::new(),
            desired_counts: Vec::new(),
            run_count_buf: Vec::new(),
            weight_buffer: Vec::new(),
            stats_counts: Vec::new(),
            stats_lengths: Vec::new(),
            histogram_key_buffer: Vec::new(),
        }
    }

    /// Produces a bit pattern matching the requested length, avoiding duplicates.
    pub(crate) fn generate<R: Rng + ?Sized>(&mut self, rng: &mut R, bits: u32) -> u128 {
        assert!((1..=128).contains(&bits), "bits must be between 1 and 128");
        let target_bits = bits as usize;

        self.candidates.clear();
        self.evaluations.clear();
        self.histogram_best.clear();
        self.batch_keys.clear();

        let mut attempts = 0;
        let mut desired_counts = std::mem::take(&mut self.desired_counts);

        while self.evaluations.len() < self.params.candidate_batch_size
            && attempts < self.params.max_plan_attempts
        {
            attempts += 1;
            let plan = match self.sample_run_plan(target_bits, rng) {
                Some(plan) => plan,
                None => continue,
            };

            desired_counts.clear();
            desired_counts.extend(plan.counts.iter().map(|&value| value as f64));

            if let Some(pattern) =
                self.build_sequence(rng, target_bits, desired_counts.as_mut_slice())
            {
                let key = BitPatternKey::from(&pattern);
                if self.seen.contains(&key) || !self.batch_keys.insert(key) {
                    continue;
                }

                let evaluation = self.evaluate_candidate(pattern, &plan, target_bits);
                self.evaluations.push(evaluation);
            }
        }

        let result = if let Some(best) = self.evaluations.iter().min_by(|a, b| {
            a.total_score
                .partial_cmp(&b.total_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            let best_pattern = best.pattern.clone();
            let key = BitPatternKey::from(&best_pattern);
            self.seen.insert(key);
            best_pattern.value
        } else {
            let fallback = self.fallback_sequence(rng, target_bits);
            let key = BitPatternKey::from(&fallback);
            self.seen.insert(key);
            fallback.value
        };

        self.desired_counts = desired_counts;
        result
    }

    /// Samples an integer run plan that obeys alternating parity and length tolerances.
    fn sample_run_plan<R: Rng + ?Sized>(
        &mut self,
        target_bits: usize,
        rng: &mut R,
    ) -> Option<RunPlan> {
        let expected_runs = self.model.scaled_expected_runs(target_bits);
        let base_mean = expected_runs
            .iter()
            .copied()
            .sum::<f64>()
            .max(self.params.total_run_mean);
        let normal = Normal::new(base_mean, self.params.total_run_sigma)
            .unwrap_or_else(|_| Normal::new(base_mean, 1.0).expect("invalid normal"));

        let min_len = self
            .model
            .min_len_for_bit
            .iter()
            .copied()
            .min()
            .unwrap_or(1)
            .max(1);
        let max_runs = (target_bits / min_len).max(2);

        let tolerance = self.params.plan_length_tolerance as isize;
        let min_bits = target_bits as isize - tolerance;
        let max_bits = target_bits as isize + tolerance;

        let mut attempts = 0;
        let mut run_counts = std::mem::take(&mut self.run_count_buf);
        let mut weights = std::mem::take(&mut self.weight_buffer);
        let mut plan = None;

        while attempts < self.params.max_plan_attempts {
            attempts += 1;
            let probabilities = self.sample_dirichlet(&expected_runs, rng);
            let mut total_runs = normal.sample(rng).round() as isize;
            if total_runs < 2 {
                total_runs = 2;
            }
            if total_runs as usize > max_runs {
                total_runs = max_runs as isize;
            }

            let ones_runs = ((total_runs + 1) / 2) as usize;
            let zeros_runs = (total_runs / 2) as usize;

            run_counts.clear();
            run_counts.resize(probabilities.len(), 0);

            if !Self::allocate_runs(
                &mut weights,
                ones_runs,
                &self.model.ones_indices,
                &probabilities,
                &mut run_counts,
                rng,
            ) {
                continue;
            }

            if !Self::allocate_runs(
                &mut weights,
                zeros_runs,
                &self.model.zeros_indices,
                &probabilities,
                &mut run_counts,
                rng,
            ) {
                continue;
            }

            let bit_estimate: usize = run_counts
                .iter()
                .enumerate()
                .map(|(idx, &count)| count * self.model.categories[idx].len as usize)
                .sum();

            let bits_estimate_signed = bit_estimate as isize;
            if bits_estimate_signed < min_bits || bits_estimate_signed > max_bits {
                continue;
            }

            plan = Some(RunPlan {
                counts: run_counts.clone(),
            });
            break;
        }

        self.run_count_buf = run_counts;
        self.weight_buffer = weights;
        plan
    }

    /// Blends analytics priors with sampled intentions for each run category.
    fn sample_dirichlet<R: Rng + ?Sized>(&self, expected_runs: &[f64], rng: &mut R) -> Vec<f64> {
        let expected_total = expected_runs
            .iter()
            .copied()
            .sum::<f64>()
            .max(DIRICHLET_EPSILON);
        let concentration = (expected_total / self.params.dirichlet_prior_scale)
            .max(self.params.dirichlet_min_concentration);

        let global_total = self
            .model
            .global_run_counts
            .iter()
            .copied()
            .sum::<f64>()
            .max(1.0);

        let mut alphas = Vec::with_capacity(expected_runs.len());
        for (idx, &expected) in expected_runs.iter().enumerate() {
            let prior_fraction = self.model.global_run_counts[idx] / global_total;
            let alpha = expected.max(DIRICHLET_EPSILON) + concentration * prior_fraction;
            alphas.push(alpha.max(DIRICHLET_EPSILON));
        }

        dirichlet_sample(&alphas, rng)
    }

    fn fallback_weight(&self, idx: usize) -> f64 {
        let bit = self.model.categories[idx].bit;
        let indices = if bit == 1 {
            &self.model.ones_indices
        } else {
            &self.model.zeros_indices
        };

        if indices.is_empty() {
            return 1.0;
        }

        let mut sum = 0.0;
        for &candidate in indices {
            sum += self.model.global_run_counts[candidate];
        }

        if sum <= 0.0 {
            1.0 / indices.len() as f64
        } else {
            self.model.global_run_counts[idx].max(WEIGHT_EPSILON) / sum
        }
    }

    fn allocate_runs<R: Rng + ?Sized>(
        weights: &mut Vec<f64>,
        draws: usize,
        indices: &[usize],
        probabilities: &[f64],
        counts: &mut [usize],
        rng: &mut R,
    ) -> bool {
        if draws == 0 {
            return true;
        }
        if indices.is_empty() {
            return false;
        }

        weights.clear();
        weights.reserve(indices.len());
        for &idx in indices {
            weights.push(probabilities[idx].max(WEIGHT_EPSILON));
        }

        let sum = weights.iter().sum::<f64>();
        if sum <= 0.0 {
            for weight in weights.iter_mut() {
                *weight = 1.0;
            }
        }

        let dist = WeightedIndex::new(&*weights).expect("invalid weight distribution");

        for _ in 0..draws {
            let selection = indices[dist.sample(rng)];
            counts[selection] += 1;
        }

        true
    }

    /// Attempts to assemble a full bit pattern using recursive DFS.
    fn build_sequence<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
        target_bits: usize,
        desired_counts: &mut [f64],
    ) -> Option<BitPattern> {
        let mut budget = SearchBudget::new(self.params.max_nodes);
        let mut state = SequenceState::new();
        self.recursive_build(
            rng,
            None,
            desired_counts,
            target_bits,
            &mut state,
            0,
            &mut budget,
        )
    }

    /// Depth-first exploration of the run space with pruning heuristics.
    fn recursive_build<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
        prev_idx: Option<usize>,
        remaining_counts: &mut [f64],
        target_bits: usize,
        state: &mut SequenceState,
        depth: usize,
        budget: &mut SearchBudget,
    ) -> Option<BitPattern> {
        if !budget.hit() {
            return None;
        }

        if state.bit_length() == target_bits {
            return Some(state.to_pattern());
        }

        if state.bit_length() > target_bits || depth > target_bits * 2 {
            return None;
        }

        let next_bit = state.next_bit();
        self.collect_candidates(
            prev_idx,
            next_bit,
            remaining_counts,
            target_bits,
            state.bit_length(),
        );

        if self.candidates.is_empty() {
            return None;
        }

        while !self.candidates.is_empty() {
            let idx_choice = sample_candidate_index(rng, &self.candidates);
            let candidate = self.candidates.swap_remove(idx_choice);

            let previous = remaining_counts[candidate.idx];
            remaining_counts[candidate.idx] = candidate.value_after;

            if !state.push_run(
                candidate.idx,
                candidate.length_used,
                candidate.fallback_used,
                candidate.truncated,
            ) {
                remaining_counts[candidate.idx] = previous;
                continue;
            }

            let result = self.recursive_build(
                rng,
                Some(candidate.idx),
                remaining_counts,
                target_bits,
                state,
                depth + 1,
                budget,
            );

            if let Some(pattern) = result {
                return Some(pattern);
            }

            state.pop_run();
            remaining_counts[candidate.idx] = previous;
        }

        None
    }

    /// Collects viable next runs and scores them using transition probabilities.
    fn collect_candidates(
        &mut self,
        prev_idx: Option<usize>,
        next_bit: u8,
        remaining_counts: &[f64],
        target_bits: usize,
        produced_bits: usize,
    ) {
        self.candidates.clear();

        for (idx, category) in self.model.categories.iter().enumerate() {
            if category.bit != next_bit {
                continue;
            }

            let length_full = category.len as usize;
            let final_length = if produced_bits + length_full >= target_bits {
                target_bits.saturating_sub(produced_bits)
            } else {
                length_full
            };

            if final_length == 0 {
                continue;
            }

            let transition = if let Some(prev) = prev_idx {
                self.model.transitions[prev][idx]
            } else {
                self.model.start_probs[idx]
            }
            .max(WEIGHT_EPSILON);

            let fallback_used = remaining_counts[idx] <= self.params.fallback_threshold;
            let budget_component = if fallback_used {
                self.params.fallback_budget_weight.max(WEIGHT_EPSILON)
            } else {
                (remaining_counts[idx].max(0.0) + self.params.budget_eps).powf(self.params.eta)
            };

            let next_total = produced_bits + final_length;
            let value_after = remaining_counts[idx] - 1.0;
            let is_final = next_total >= target_bits;
            let truncated = final_length != length_full;

            if is_final && !self.can_finish_after(remaining_counts, idx, value_after) {
                continue;
            }

            let balance = self.balance_penalty_adjusted(
                idx,
                produced_bits,
                final_length,
                target_bits,
                remaining_counts,
                value_after,
                is_final,
            );

            let mut score = transition.powf(self.params.gamma) * budget_component * balance;

            if fallback_used {
                score *= self.params.fallback_transition_penalty;
                score *= self.fallback_weight(idx);
            }

            if is_final {
                score *= self.model.terminal_probs[idx].max(WEIGHT_EPSILON).sqrt();
            }

            if score > 0.0 {
                self.candidates.push(Candidate {
                    idx,
                    score,
                    length_used: final_length as u8,
                    value_after,
                    fallback_used,
                    truncated,
                });
            }
        }
        if self.candidates.len() > self.params.max_candidates_per_step {
            self.candidates
                .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
            self.candidates
                .truncate(self.params.max_candidates_per_step);
        }
    }

    fn evaluate_candidate(
        &mut self,
        pattern: BitPattern,
        plan: &RunPlan,
        target_bits: usize,
    ) -> EvaluatedPattern {
        let stats = self.compute_stats(&pattern, target_bits);
        let PatternStats {
            counts,
            lengths,
            fallback_runs,
            truncated_runs,
            log_likelihood,
            histogram_key,
        } = stats;

        let expected_runs = self.model.scaled_expected_runs(target_bits);
        let target_shares = self.model.share_target();
        let share_weight = self.params.share_weight;
        let count_penalty_weight = self.params.count_penalty_weight;
        let fallback_penalty_weight = self.params.fallback_penalty_weight;
        let trunc_penalty_weight = self.params.trunc_penalty_weight;
        let length_penalty_weight = self.params.length_penalty_weight;
        let novelty_epsilon = self.params.novelty_epsilon;
        let novelty_penalty_weight = self.params.novelty_penalty_weight;
        let total_bits = pattern.length.max(1) as f64;

        let mut share_distance = 0.0;
        for (idx, &length) in lengths.iter().enumerate() {
            let actual_share = length as f64 / total_bits;
            let target_share = target_shares.get(idx).copied().unwrap_or(0.0);
            let weight = expected_runs
                .get(idx)
                .copied()
                .unwrap_or(0.0)
                .max(WEIGHT_EPSILON);
            share_distance += weight * (actual_share - target_share).abs();
        }

        let mut count_penalty_raw = 0.0;
        for (idx, &planned) in plan.counts.iter().enumerate() {
            let actual = counts.get(idx).copied().unwrap_or(0) as isize;
            let goal = planned as isize;
            let diff = (actual - goal).abs();
            if diff > 1 {
                count_penalty_raw += (diff - 1) as f64;
            }
        }
        let count_penalty = count_penalty_raw * count_penalty_weight;

        let fallback_penalty = fallback_runs as f64 * fallback_penalty_weight;
        let trunc_penalty = truncated_runs as f64 * trunc_penalty_weight;
        let length_penalty = if pattern.length as usize == target_bits {
            0.0
        } else {
            (pattern.length as isize - target_bits as isize).abs() as f64 * length_penalty_weight
        };

        let histogram_key_slice = histogram_key.as_slice();
        let mut novelty_penalty = 0.0;
        if let Some(best_log) = self.histogram_best.get(histogram_key_slice) {
            if log_likelihood <= *best_log && (*best_log - log_likelihood).abs() < novelty_epsilon {
                novelty_penalty = novelty_penalty_weight;
            }
        }

        if let Some(best) = self.histogram_best.get_mut(histogram_key_slice) {
            if log_likelihood > *best {
                *best = log_likelihood;
            }
        } else {
            self.histogram_best
                .insert(histogram_key.clone(), log_likelihood);
        }

        let total_score = share_weight * share_distance
            + count_penalty
            + fallback_penalty
            + trunc_penalty
            + length_penalty
            + novelty_penalty;

        self.stats_counts = counts;
        self.stats_lengths = lengths;
        self.histogram_key_buffer = histogram_key;

        EvaluatedPattern {
            pattern,
            total_score,
        }
    }

    fn compute_stats(&mut self, pattern: &BitPattern, _target_bits: usize) -> PatternStats {
        let categories_len = self.model.categories.len();

        let mut counts = std::mem::take(&mut self.stats_counts);
        if counts.len() != categories_len {
            counts.resize(categories_len, 0);
        } else {
            for value in counts.iter_mut() {
                *value = 0;
            }
        }

        let mut lengths = std::mem::take(&mut self.stats_lengths);
        if lengths.len() != categories_len {
            lengths.resize(categories_len, 0);
        } else {
            for value in lengths.iter_mut() {
                *value = 0;
            }
        }

        let mut fallback_runs = 0usize;
        let mut truncated_runs = 0usize;
        let mut log_likelihood = 0.0;
        let mut prev_idx: Option<usize> = None;

        for segment in &pattern.runs {
            counts[segment.category_idx] += 1;
            lengths[segment.category_idx] += segment.length as usize;
            if segment.fallback {
                fallback_runs += 1;
            }
            if segment.truncated {
                truncated_runs += 1;
            }

            let weight = if let Some(prev) = prev_idx {
                self.model.transitions[prev][segment.category_idx]
            } else {
                self.model.start_probs[segment.category_idx]
            }
            .max(WEIGHT_EPSILON);

            log_likelihood += weight.ln();
            prev_idx = Some(segment.category_idx);
        }

        if let Some(last_idx) = prev_idx {
            let terminal = self.model.terminal_probs[last_idx].max(WEIGHT_EPSILON);
            log_likelihood += terminal.ln();
        }

        let mut histogram_key = std::mem::take(&mut self.histogram_key_buffer);
        histogram_key.clear();
        histogram_key.reserve(counts.len());
        for &count in &counts {
            histogram_key.push(count.min(u8::MAX as usize) as u8);
        }

        PatternStats {
            counts,
            lengths,
            fallback_runs,
            truncated_runs,
            log_likelihood,
            histogram_key,
        }
    }

    /// Penalises choices that would force the search to overshoot future budgets.
    fn balance_penalty_adjusted(
        &self,
        idx: usize,
        produced_bits: usize,
        length: usize,
        target_bits: usize,
        counts: &[f64],
        value_after: f64,
        is_final: bool,
    ) -> f64 {
        let mut penalty = 1.0;
        let new_total = produced_bits + length;

        if new_total > target_bits {
            let over = new_total - target_bits;
            penalty *= 1.0 / (1.0 + over as f64);
        }

        let mut ones_remaining = 0usize;
        let mut zeros_remaining = 0usize;
        for (cat_idx, category) in self.model.categories.iter().enumerate() {
            let adjusted = if cat_idx == idx {
                value_after
            } else {
                counts[cat_idx]
            };
            if adjusted > 0.0 {
                let ceiled = adjusted.ceil() as usize;
                if category.bit == 1 {
                    ones_remaining += ceiled;
                } else {
                    zeros_remaining += ceiled;
                }
            }
        }

        let min_bits_required = ones_remaining * self.model.min_len_for_bit[1]
            + zeros_remaining * self.model.min_len_for_bit[0];

        if !is_final && new_total + min_bits_required > target_bits + self.params.trim_allowance {
            penalty *= self.params.over_budget_penalty;
        }

        if is_final && min_bits_required > self.params.trim_allowance {
            penalty *= self.params.over_budget_penalty;
        }

        if self.model.categories[idx].bit == 0 && produced_bits == 0 {
            penalty *= 0.1;
        }

        penalty.max(WEIGHT_EPSILON)
    }

    /// Ensures that finishing moves do not violate the remaining-category budget.
    fn can_finish_after(&self, counts: &[f64], idx: usize, value_after: f64) -> bool {
        let mut remaining = 0.0;
        for (cat_idx, &value) in counts.iter().enumerate() {
            let adjusted = if cat_idx == idx { value_after } else { value };
            if adjusted > 0.0 {
                remaining += adjusted;
            }
        }
        remaining <= self.params.finish_threshold
    }

    /// Backup sampler that keeps alternating runs but drops the smarter heuristics.
    fn fallback_sequence<R: Rng + ?Sized>(&self, rng: &mut R, bits: usize) -> BitPattern {
        let mut state = SequenceState::new();

        while state.bit_length() < bits {
            let next_bit = state.next_bit();
            let indices = if next_bit == 1 {
                &self.model.ones_indices
            } else {
                &self.model.zeros_indices
            };

            if indices.is_empty() {
                break;
            }

            let idx = indices[rng.gen_range(0..indices.len())];
            let length = self.model.categories[idx].len as usize;
            let remaining = bits - state.bit_length();
            let span = length.min(remaining);

            let truncated = span < length;
            if span == 0 || !state.push_run(idx, span as u8, false, truncated) {
                break;
            }
        }

        state.to_pattern()
    }
}

/// Candidate descriptor produced during DFS scoring.
struct Candidate {
    idx: usize,
    score: f64,
    length_used: u8,
    value_after: f64,
    fallback_used: bool,
    truncated: bool,
}

struct EvaluatedPattern {
    pattern: BitPattern,
    total_score: f64,
}

struct PatternStats {
    counts: Vec<usize>,
    lengths: Vec<usize>,
    fallback_runs: usize,
    truncated_runs: usize,
    log_likelihood: f64,
    histogram_key: Vec<u8>,
}

struct RunPlan {
    counts: Vec<usize>,
}

/// Hashable representation of a bit pattern for deduplication.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct BitPatternKey {
    value: u128,
    length: u8,
}

/// Compact value/length pair for generated bit sequences.
#[derive(Clone)]
pub(crate) struct BitPattern {
    pub(crate) value: u128,
    pub(crate) length: u8,
    pub(crate) runs: SmallVec<[RunSegment; INLINE_RUN_CAPACITY]>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct RunSegment {
    pub(crate) category_idx: usize,
    pub(crate) length: u8,
    pub(crate) fallback: bool,
    pub(crate) truncated: bool,
}

impl From<&BitPattern> for BitPatternKey {
    fn from(pattern: &BitPattern) -> Self {
        BitPatternKey {
            value: pattern.value,
            length: pattern.length,
        }
    }
}

impl From<BitPattern> for BitPatternKey {
    fn from(pattern: BitPattern) -> Self {
        BitPatternKey::from(&pattern)
    }
}

const MAX_RUNS: usize = 128;
const INLINE_RUN_CAPACITY: usize = 32;

/// Keeps track of the partial runs while building a candidate sequence.
struct SequenceState {
    lengths: [u8; MAX_RUNS],
    values: [u128; MAX_RUNS + 1],
    records: [RunSegment; MAX_RUNS],
    total_bits: usize,
    run_count: usize,
}

impl SequenceState {
    fn new() -> Self {
        SequenceState {
            lengths: [0; MAX_RUNS],
            values: [0; MAX_RUNS + 1],
            records: [RunSegment::default(); MAX_RUNS],
            total_bits: 0,
            run_count: 0,
        }
    }

    fn next_bit(&self) -> u8 {
        if self.run_count % 2 == 0 { 1 } else { 0 }
    }

    fn push_run(&mut self, category_idx: usize, len: u8, fallback: bool, truncated: bool) -> bool {
        if len == 0 || self.run_count >= MAX_RUNS {
            return false;
        }
        let len_usize = len as usize;
        if self.total_bits + len_usize > 128 {
            return false;
        }
        let prev_value = self.values[self.run_count];
        let bit = self.next_bit();
        let shifted = prev_value.checked_shl(len as u32).unwrap_or(0);
        let mask = if bit == 1 { ones_mask(len_usize) } else { 0 };
        self.values[self.run_count + 1] = shifted | mask;
        self.lengths[self.run_count] = len;
        self.records[self.run_count] = RunSegment {
            category_idx,
            length: len,
            fallback,
            truncated,
        };
        self.run_count += 1;
        self.total_bits += len_usize;
        true
    }

    fn pop_run(&mut self) {
        if self.run_count == 0 {
            return;
        }
        self.run_count -= 1;
        self.total_bits -= self.lengths[self.run_count] as usize;
        self.records[self.run_count] = RunSegment::default();
    }

    fn bit_length(&self) -> usize {
        self.total_bits
    }

    fn to_pattern(&self) -> BitPattern {
        let mut runs = SmallVec::<[RunSegment; INLINE_RUN_CAPACITY]>::with_capacity(self.run_count);
        runs.extend_from_slice(&self.records[..self.run_count]);
        BitPattern {
            value: self.values[self.run_count],
            length: self.total_bits as u8,
            runs,
        }
    }
}

/// Precomputed mask of ones that fits within a u128.
fn ones_mask(len: usize) -> u128 {
    match len {
        0 => 0,
        128 => u128::MAX,
        1..=127 => (1u128 << len) - 1,
        _ => u128::MAX,
    }
}

/// Samples a candidate index using weighted roulette-wheel selection.
fn sample_candidate_index<R: Rng + ?Sized>(rng: &mut R, candidates: &[Candidate]) -> usize {
    let mut total = 0.0;
    for candidate in candidates {
        total += candidate.score.max(WEIGHT_EPSILON);
    }

    if total <= 0.0 {
        return rng.gen_range(0..candidates.len());
    }

    let mut draw = rng.gen_range(0.0..total);
    for (idx, candidate) in candidates.iter().enumerate() {
        draw -= candidate.score.max(WEIGHT_EPSILON);
        if draw <= 0.0 {
            return idx;
        }
    }

    candidates.len() - 1
}

/// Tunable knobs that shape the search heuristics.
#[derive(Clone)]
struct GenerationParams {
    gamma: f64,
    eta: f64,
    dirichlet_prior_scale: f64,
    dirichlet_min_concentration: f64,
    total_run_mean: f64,
    total_run_sigma: f64,
    trim_allowance: usize,
    over_budget_penalty: f64,
    finish_threshold: f64,
    budget_eps: f64,
    fallback_threshold: f64,
    fallback_budget_weight: f64,
    fallback_transition_penalty: f64,
    share_weight: f64,
    count_penalty_weight: f64,
    fallback_penalty_weight: f64,
    trunc_penalty_weight: f64,
    novelty_penalty_weight: f64,
    novelty_epsilon: f64,
    length_penalty_weight: f64,
    plan_length_tolerance: usize,
    candidate_batch_size: usize,
    max_candidates_per_step: usize,
    max_plan_attempts: usize,
    max_nodes: usize,
}

impl Default for GenerationParams {
    fn default() -> Self {
        GenerationParams {
            gamma: 1.0,
            eta: 0.8,
            dirichlet_prior_scale: 4.0,
            dirichlet_min_concentration: 1.0,
            total_run_mean: 35.0,
            total_run_sigma: 5.0,
            trim_allowance: 2,
            over_budget_penalty: 0.2,
            finish_threshold: 0.75,
            budget_eps: 0.25,
            fallback_threshold: 0.0,
            fallback_budget_weight: 0.1,
            fallback_transition_penalty: 0.5,
            share_weight: 1.0,
            count_penalty_weight: 1.5,
            fallback_penalty_weight: 0.75,
            trunc_penalty_weight: 0.5,
            novelty_penalty_weight: 0.25,
            novelty_epsilon: 0.1,
            length_penalty_weight: 2.0,
            plan_length_tolerance: 2,
            candidate_batch_size: 200,
            max_candidates_per_step: 5,
            max_plan_attempts: 800,
            max_nodes: 8000,
        }
    }
}

/// Cheap node counter that guards against runaway DFS.
struct SearchBudget {
    nodes: usize,
    limit: usize,
}

impl SearchBudget {
    fn new(limit: usize) -> Self {
        SearchBudget { nodes: 0, limit }
    }

    fn hit(&mut self) -> bool {
        if self.nodes >= self.limit {
            false
        } else {
            self.nodes += 1;
            true
        }
    }
}
