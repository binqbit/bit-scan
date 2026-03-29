//! Analytics-backed metadata loader for scan v2.
//!
//! The `Model` aggregates run-length statistics, transition weights, and
//! normalisation helpers derived from the historical analytics dump. The
//! generator queries this structure to stay aligned with the observed wallet
//! patterns without hitting disk again.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use super::{
    constants::{
        ANALYTICS_DIR, SHARE_ALPHA, SHARE_BETA, START_SMOOTHING, TARGET_BASE_LENGTH,
        TERMINAL_SMOOTHING, TRANSITION_SMOOTHING,
    },
    probability::normalize,
};

/// Runtime view of the analytics corpus used to steer generation.
pub(crate) struct Model {
    pub(crate) categories: Vec<RunCategory>,
    pub(crate) start_probs: Vec<f64>,
    pub(crate) terminal_probs: Vec<f64>,
    pub(crate) transitions: Vec<Vec<f64>>,
    pub(crate) global_run_counts: Vec<f64>,
    pub(crate) expected_runs_71: Vec<f64>,
    pub(crate) share_target: Vec<f64>,
    pub(crate) min_len_for_bit: [usize; 2],
    pub(crate) ones_indices: Vec<usize>,
    pub(crate) zeros_indices: Vec<usize>,
}

impl Model {
    /// Loads the analytics snapshot from disk and converts it into runtime
    /// structures that the generator can query efficiently.
    pub(crate) fn load() -> Result<Self, String> {
        let base_path = PathBuf::from(ANALYTICS_DIR);
        let ratios = NumericTable::from_file(base_path.join("bit_block_ratios.txt"))?;
        let ratio_changes = NumericTable::from_file(base_path.join("bit_block_ratio_changes.txt"))?;

        if ratios.header != ratio_changes.header {
            return Err("ratio headers mismatch".into());
        }

        let categories = parse_run_categories(&ratios.header)?;
        let index_map = build_index_map(&categories);

        let regression_projection = regression_projection(&ratios.rows);
        let s70 = ratios
            .row_by_line(70)
            .ok_or_else(|| "missing line 70 ratios".to_string())?;
        let d70 = ratio_changes
            .row_by_line(70)
            .ok_or_else(|| "missing line 70 ratio deltas".to_string())?;

        let mut projected = Vec::with_capacity(categories.len());
        let mut momentum = Vec::with_capacity(categories.len());
        for idx in 0..categories.len() {
            let reg = regression_projection[idx].max(0.0);
            projected.push(reg);
            momentum.push((s70.values[idx] + SHARE_BETA * d70.values[idx]).max(0.0));
        }

        let mut blended: Vec<f64> = projected
            .iter()
            .zip(momentum.iter())
            .map(|(reg, mom)| SHARE_ALPHA * reg + (1.0 - SHARE_ALPHA) * mom)
            .map(|v| v.max(0.0))
            .collect();
        normalize(&mut blended);

        let share_target = blended.clone();

        let expected_runs_71 = share_target
            .iter()
            .enumerate()
            .map(|(idx, share)| {
                let length = categories[idx].len.max(1) as f64;
                share * TARGET_BASE_LENGTH as f64 / length
            })
            .collect::<Vec<f64>>();

        let run_stats = build_run_stats(&categories, &index_map)?;
        let mut global_run_counts = run_stats.run_counts_total.clone();
        accumulate_run_totals(
            base_path.join("1_bits.txt"),
            1,
            &index_map,
            &mut global_run_counts,
        )?;
        accumulate_run_totals(
            base_path.join("0_bits.txt"),
            0,
            &index_map,
            &mut global_run_counts,
        )?;

        let ones_indices = categories
            .iter()
            .enumerate()
            .filter_map(|(idx, cat)| (cat.bit == 1).then_some(idx))
            .collect::<Vec<usize>>();
        let zeros_indices = categories
            .iter()
            .enumerate()
            .filter_map(|(idx, cat)| (cat.bit == 0).then_some(idx))
            .collect::<Vec<usize>>();

        let start_probs =
            smooth_distribution(&run_stats.start_counts, &ones_indices, START_SMOOTHING);
        let terminal_probs = smooth_distribution(
            &run_stats.terminal_counts,
            &zeros_indices,
            TERMINAL_SMOOTHING,
        );
        let transitions = normalize_transitions(
            &run_stats.transition_counts,
            &categories,
            TRANSITION_SMOOTHING,
        );

        let mut min_len_for_bit = [usize::MAX; 2];
        for cat in &categories {
            let bit_idx = cat.bit as usize;
            let length = cat.len as usize;
            if length < min_len_for_bit[bit_idx] {
                min_len_for_bit[bit_idx] = length;
            }
        }
        for value in min_len_for_bit.iter_mut() {
            if *value == usize::MAX {
                *value = 1;
            }
        }

        Ok(Model {
            categories,
            start_probs,
            terminal_probs,
            transitions,
            global_run_counts,
            expected_runs_71,
            share_target,
            min_len_for_bit,
            ones_indices,
            zeros_indices,
        })
    }

    /// Scales the expected run counts from the 71-bit baseline to the requested target.
    pub(crate) fn share_target(&self) -> &[f64] {
        &self.share_target
    }

    pub(crate) fn scaled_expected_runs(&self, target_bits: usize) -> Vec<f64> {
        let scale = target_bits as f64 / TARGET_BASE_LENGTH as f64;
        self.expected_runs_71
            .iter()
            .map(|v| (v * scale).max(super::constants::WEIGHT_EPSILON))
            .collect()
    }
}

/// Canonical representation of a run category from the analytics tables.
#[derive(Clone)]
pub(crate) struct RunCategory {
    pub(crate) bit: u8,
    pub(crate) len: u8,
}

struct RunStats {
    start_counts: Vec<f64>,
    terminal_counts: Vec<f64>,
    transition_counts: Vec<Vec<f64>>,
    run_counts_total: Vec<f64>,
}

struct NumericTable {
    header: Vec<String>,
    rows: Vec<TableRow>,
}

struct TableRow {
    line: usize,
    values: Vec<f64>,
}

impl NumericTable {
    /// Loads a CSV snapshot into memory, preserving the header column order.
    fn from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("failed to read {}: {}", path.as_ref().display(), e))?;
        let mut lines = content.lines();
        let header_line = lines.next().ok_or("missing header")?;
        let mut header_parts: Vec<String> = header_line
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        if header_parts.is_empty() {
            return Err("empty header".into());
        }
        header_parts.remove(0); // drop "line"

        let mut rows = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parts: Vec<&str> = trimmed.split(',').collect();
            if parts.len() < 2 {
                continue;
            }
            let line_idx = parts[0]
                .parse::<usize>()
                .map_err(|e| format!("invalid line index '{}': {}", parts[0], e))?;
            let mut values = Vec::with_capacity(header_parts.len());
            for value_str in parts.iter().skip(1) {
                let parsed = value_str
                    .parse::<f64>()
                    .map_err(|e| format!("invalid float '{}': {}", value_str, e))?;
                values.push(parsed);
            }
            while values.len() < header_parts.len() {
                values.push(0.0);
            }
            rows.push(TableRow {
                line: line_idx,
                values,
            });
        }

        Ok(NumericTable {
            header: header_parts,
            rows,
        })
    }

    fn row_by_line(&self, line: usize) -> Option<&TableRow> {
        self.rows.iter().find(|row| row.line == line)
    }
}

fn parse_run_categories(header: &[String]) -> Result<Vec<RunCategory>, String> {
    let mut categories = Vec::with_capacity(header.len());
    for label in header {
        let parts: Vec<&str> = label.split('x').collect();
        if parts.len() != 2 {
            return Err(format!("invalid run label: {}", label));
        }
        let bit = parts[0]
            .parse::<u8>()
            .map_err(|_| format!("invalid bit in label: {}", label))?;
        let len = parts[1]
            .parse::<u8>()
            .map_err(|_| format!("invalid length in label: {}", label))?;
        categories.push(RunCategory { bit, len });
    }
    Ok(categories)
}

fn build_index_map(categories: &[RunCategory]) -> HashMap<(u8, u8), usize> {
    let mut map = HashMap::new();
    for (idx, category) in categories.iter().enumerate() {
        map.insert((category.bit, category.len), idx);
    }
    map
}

fn regression_projection(rows: &[TableRow]) -> Vec<f64> {
    if rows.is_empty() {
        return Vec::new();
    }

    let n = rows.len();
    let mean_x = (n as f64 + 1.0) / 2.0;
    let var_x = (1..=n)
        .map(|x| {
            let dx = x as f64 - mean_x;
            dx * dx
        })
        .sum::<f64>()
        .max(super::constants::WEIGHT_EPSILON);

    let col_count = rows[0].values.len();
    let mut projections = vec![0.0; col_count];

    for col in 0..col_count {
        let mut mean_y = 0.0;
        for row in rows {
            mean_y += row.values[col];
        }
        mean_y /= n as f64;

        let mut cov = 0.0;
        for (idx, row) in rows.iter().enumerate() {
            let x = (idx + 1) as f64;
            cov += (x - mean_x) * (row.values[col] - mean_y);
        }
        let slope = cov / var_x;
        let intercept = mean_y - slope * mean_x;
        projections[col] = (intercept + slope * (n as f64 + 1.0)).max(0.0);
    }

    projections
}

fn build_run_stats(
    categories: &[RunCategory],
    index_map: &HashMap<(u8, u8), usize>,
) -> Result<RunStats, String> {
    let path = Path::new(ANALYTICS_DIR).join("private_keys_1_70_bit.csv");
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    let mut lines = content.lines();
    lines.next(); // skip header

    let mut start_counts = vec![0.0; categories.len()];
    let mut terminal_counts = vec![0.0; categories.len()];
    let mut transition_counts = vec![vec![0.0; categories.len()]; categories.len()];
    let mut total_counts = vec![0.0; categories.len()];

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split(',').collect();
        if parts.is_empty() {
            continue;
        }
        let bit_str = parts[0].trim();
        let trimmed_bits = bit_str.trim_start_matches('0');
        if trimmed_bits.is_empty() {
            continue;
        }
        let runs = compute_runs(trimmed_bits);
        if runs.is_empty() {
            continue;
        }

        for (bit, len) in &runs {
            if let Some(&idx) = index_map.get(&(*bit, *len)) {
                total_counts[idx] += 1.0;
            }
        }

        if let Some(&(bit, len)) = runs.first() {
            if let Some(&idx) = index_map.get(&(bit, len)) {
                start_counts[idx] += 1.0;
            }
        }

        if let Some(&(bit, len)) = runs.last() {
            if let Some(&idx) = index_map.get(&(bit, len)) {
                terminal_counts[idx] += 1.0;
            }
        }

        for window in runs.windows(2) {
            let (from_bit, from_len) = window[0];
            let (to_bit, to_len) = window[1];
            if let (Some(&from_idx), Some(&to_idx)) = (
                index_map.get(&(from_bit, from_len)),
                index_map.get(&(to_bit, to_len)),
            ) {
                transition_counts[from_idx][to_idx] += 1.0;
            }
        }
    }

    Ok(RunStats {
        start_counts,
        terminal_counts,
        transition_counts,
        run_counts_total: total_counts,
    })
}

fn compute_runs(bits: &str) -> Vec<(u8, u8)> {
    let mut chars = bits.chars();
    let mut runs = Vec::new();
    let mut current = match chars.next() {
        Some(c) => c,
        None => return runs,
    };
    let mut length: u8 = 1;

    for ch in chars {
        if ch == current {
            length = length.saturating_add(1);
        } else {
            let bit = (current as u8).saturating_sub(b'0');
            runs.push((bit, length));
            current = ch;
            length = 1;
        }
    }

    let bit = (current as u8).saturating_sub(b'0');
    runs.push((bit, length));
    runs
}

fn accumulate_run_totals(
    path: PathBuf,
    bit: u8,
    index_map: &HashMap<(u8, u8), usize>,
    totals: &mut [f64],
) -> Result<(), String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    let mut lines = content.lines();
    let header = lines.next().ok_or("missing run count header")?;
    let lengths: Vec<u8> = header
        .split(',')
        .skip(1)
        .map(|s| {
            s.trim()
                .parse::<u8>()
                .map_err(|e| format!("invalid length '{}': {}", s, e))
        })
        .collect::<Result<_, _>>()?;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split(',').collect();
        for (idx, value_str) in parts.iter().enumerate().skip(1) {
            let value = value_str
                .parse::<f64>()
                .map_err(|e| format!("invalid count '{}': {}", value_str, e))?;
            if let Some(&length) = lengths.get(idx - 1) {
                if let Some(&cat_idx) = index_map.get(&(bit, length)) {
                    totals[cat_idx] += value;
                }
            }
        }
    }

    Ok(())
}

fn smooth_distribution(counts: &[f64], eligible: &[usize], smoothing: f64) -> Vec<f64> {
    let mut probs = vec![0.0; counts.len()];
    if eligible.is_empty() {
        return probs;
    }
    let mut sum = 0.0;
    for &idx in eligible {
        let value = counts[idx] + smoothing;
        probs[idx] = value;
        sum += value;
    }
    if sum <= 0.0 {
        let uniform = 1.0 / eligible.len() as f64;
        for &idx in eligible {
            probs[idx] = uniform;
        }
    } else {
        for &idx in eligible {
            probs[idx] /= sum;
        }
    }
    probs
}

fn normalize_transitions(
    counts: &[Vec<f64>],
    categories: &[RunCategory],
    smoothing: f64,
) -> Vec<Vec<f64>> {
    let mut transitions = vec![vec![0.0; categories.len()]; categories.len()];

    for (from_idx, row) in counts.iter().enumerate() {
        let from_bit = categories[from_idx].bit;
        let mut sum = 0.0;
        for to_idx in 0..categories.len() {
            if categories[to_idx].bit == from_bit {
                continue;
            }
            let value = row[to_idx] + smoothing;
            transitions[from_idx][to_idx] = value;
            sum += value;
        }

        if sum <= 0.0 {
            let candidates: Vec<usize> = categories
                .iter()
                .enumerate()
                .filter_map(|(idx, cat)| (cat.bit != from_bit).then_some(idx))
                .collect();
            if !candidates.is_empty() {
                let uniform = 1.0 / candidates.len() as f64;
                for idx in candidates {
                    transitions[from_idx][idx] = uniform;
                }
            }
        } else {
            for to_idx in 0..categories.len() {
                if categories[to_idx].bit != from_bit {
                    transitions[from_idx][to_idx] /= sum;
                }
            }
        }
    }

    transitions
}
