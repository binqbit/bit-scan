# Wallet 71 Run-Length Prediction Algorithm

## Data Signals
- `analytics/private_keys_1_70_bit.csv`: trimmed wallet bit strings plus hex/decimal for ground-truth run extraction.
- `analytics/1_bits.txt` and `analytics/0_bits.txt`: per-wallet run-length counts for `1` and `0` blocks.
- `analytics/bit_block_ratios.txt`: run-length bit-share for each wallet; rows sum to 1.0.
- `analytics/bit_block_ratio_changes.txt`: per-wallet deltas in those shares, capturing how compositions shift over time.

### Aggregate run-length share and trend
Averages and linear trends across the first 70 wallets give the baseline. Regression is simple least-squares over wallet index -> share. The 71-bit trimmed length is consistent with the historical slope (=1 bit per wallet).

| run | mean share | slope per wallet | predicted share (w71) | expected runs (w71) |
|-----|------------|------------------|-----------------------|---------------------|
| 1x1 | 0.159 | -0.0018 | 0.094 | 6.65 |
| 1x2 | 0.149 | -0.0008 | 0.119 | 4.23 |
| 1x3 | 0.098 | -0.0005 | 0.079 | 1.88 |
| 1x4 | 0.056 | 0.0006 | 0.077 | 1.37 |
| 1x5 | 0.030 | 0.0007 | 0.056 | 0.79 |
| 1x6 | 0.023 | 0.0007 | 0.049 | 0.58 |
| 1x7 | 0.003 | -0.0000 | 0.002 | 0.02 |
| 1x10 | 0.003 | 0.0001 | 0.007 | 0.05 |
| 0x1 | 0.125 | 0.0008 | 0.152 | 10.80 |
| 0x2 | 0.124 | -0.0000 | 0.123 | 4.38 |
| 0x3 | 0.083 | -0.0002 | 0.076 | 1.79 |
| 0x4 | 0.051 | 0.0010 | 0.086 | 1.53 |
| 0x5 | 0.049 | -0.0001 | 0.047 | 0.67 |
| 0x6 | 0.011 | -0.0001 | 0.007 | 0.08 |
| 0x7 | 0.027 | -0.0004 | 0.014 | 0.14 |
| 0x8 | 0.007 | -0.0000 | 0.006 | 0.06 |
| 0x9 | 0.003 | 0.0001 | 0.006 | 0.05 |

Run occurrences aggregated across 70 wallets (from `private_keys_1_70_bit.csv`):
- Ones runs: length 1 (51.6%), 2 (26.0%), 3 (11.4%), 4 (5.9%), 5 (2.9%), 6 (2.0%), 7 (0.15%), 10 (0.15%).
- Zero runs: length 1 (52.6%), 2 (24.4%), 3 (10.6%), 4 (6.2%), 5 (3.8%), 6 (0.6%), 7 (1.3%), 8 (0.3%), 9 (0.16%).

Transition tendencies (alternating runs from the same source):
- After `1x1`: `0x1` 51.5%, `0x2` 25.9%, `0x3` 11.3%, longer zeros make up the remaining 11.3%.
- After `1x2`: `0x1` 53.0%, `0x2` 25.6%, `0x3` 11.0%.
- After `0x1`: `1x1` 53.3%, `1x2` 26.0%, `1x3` 12.2%.
- After `0x2`: `1x1` 52.1%, `1x2` 25.7%, `1x3` 10.0%, longer ones supply the balance.

Additional context:
- Start-of-line runs are `1x1` 48.6%, `1x2` 28.6%, `1x3` 12.9%, with occasional longer bursts.
- Average trimmed length grew linearly to 70 bits; last-20 wallets average 60.5 bits, last-10 average 65.5 bits.
- Last recorded share jump (row 70 in `bit_block_ratio_changes.txt`) spikes `1x2` (+0.1126) and mid-length zero runs `0x4` (+0.1143), while collapsing `1x6` and `0x7`. This momentum is useful for short-term biasing.

## Predictive Generation Strategy
The goal is to generate a 71-bit trimmed sequence whose run profile is statistically close to the observed distribution while embracing randomness. The process combines long-term averages, trend projections, and local transition structure.

### Step 1: Target composition for wallet 71
1. Pull the latest share vector `s70` (`bit_block_ratios` row 70) and the most recent change `d70` (`bit_block_ratio_changes` row 70).
2. Compute the regression-based projection `s_reg` (table above).
3. Blend the two to capture both drift and momentum: `s_target = normalize(alpha * s_reg + (1 - alpha) * (s70 + beta * d70))`, with `alpha ~ 0.6` and `beta ~ 1.0` working well in backtests. Clamp negatives to zero before renormalizing.
4. Convert shares into expected run counts using the 71-bit target length: `expected_runs[r] = s_target[r] * 71 / run_length(r)`.

### Step 2: Sample an integer run budget
1. Treat `expected_runs` as the mean of a Dirichlet-multinomial. Use pseudocounts proportional to aggregate run frequencies (`1_bits` / `0_bits`) to avoid zero-probability categories.
2. Sample a concrete run-count vector `c[r]` that alternates by construction (the total number of `1x*` runs should differ from `0x*` by at most one so the sequence can start with a `1` run).
3. Draw the total number of runs from a truncated normal with `mu = 35`, `sigma = 5` (matching recent wallets), conditioned to keep the total bit length within +/- 2 bits of 71 when respecting `c[r]`.

### Step 3: Generate an alternating run sequence
1. Determine the initial run by sampling from the start distribution, weighted by the remaining budget `c[r]`. Example weight: `w(r) = start_prob(r) * (max(c[r], eps))^eta`, with `eta ~ 0.8`.
2. For each subsequent run, use the empirical transition matrix `P(prev -> next)` (computed from `private_keys_1_70_bit.csv`). Scale each candidate by the residual budget:
   `score(next) = P(prev -> next)^gamma * (max(c[next], eps))^eta * balance_penalty(next)`
   - `gamma` near 1 preserves the empirical shape.
   - `balance_penalty` discourages choosing a run that would make the remaining slots impossible (for example, avoid a long zero when only short ones remain in the budget).
3. After selecting a run, append `bit * run_length` to the sequence, decrement `c[run]`, and continue. If a required category is depleted, fall back to the global run distribution for that bit type, but flag a penalty for scoring later.
4. Maintain the current bit-length total. If the final run would overshoot 71 bits, truncate that run and note the adjustment for scoring.

### Step 4: Score and select candidates
1. Generate `N` candidates (e.g., 200). For each, recompute actual run shares and compare to `s_target` using a weighted L1 distance.
2. Penalize violations: use extra cost for each category that deviates by more than 1 run from `c[r]`, for truncations, or for impossible transitions that required fallback sampling.
3. Pick the sequence with the lowest total score; keep a short list of top candidates if downstream heuristics need options.

### Step 5: Output and validation
- Store the winning run-length list alongside the 71-bit string. Re-run the analysis pipeline to confirm its run counts and shares fall inside the historical interquartile ranges.
- Optionally compute the log-likelihood of the run sequence under the transition model to surface outliers.

## Implementation Notes
- Recompute all statistics directly from the CSV/TXT inputs before each prediction to stay aligned with future data refreshes.
- The Dirichlet concentration can be tuned: higher values hug `expected_runs`, lower values create more spread. Start with concentration ~ total_runs / 4.
- If momentum (`beta`) produces negative shares, shrink `beta` and renormalize.
- For reproducibility, seed the RNG but allow the caller to vary the seed to explore alternate wallet candidates.
- Once the 71st wallet is published, append it to the dataset and retrain the regression/transition matrices before attempting wallet 72.
