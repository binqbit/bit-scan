# 71-Bit Wallet Generation Algorithm

## Purpose
This note distils the empirical findings in analytics/bits_statistic.md into the executable steps required to synthesise 71-bit wallet candidates. The procedure blends the long-run distributions from the first 70 wallets with the momentum visible in the most recent samples so that each generated sequence mirrors the statistical fingerprint expected for wallet 71.

## Data Inputs
- analytics/private_keys_1_70_bit.csv - source bit strings used to rebuild run-length sequences.
- analytics/1_bits.txt and analytics/0_bits.txt - historical run counts for ones and zeros.
- analytics/bit_block_ratios.txt - per-wallet run-length bit shares (rows sum to 1).
- analytics/bit_block_ratio_changes.txt - first differences of those shares, capturing short-term drift.
- analytics/bits_statistic.md - summary statistics for expected run counts, probabilities, and transitions that anchor the 71-bit forecast.

Reload these artefacts before every generation cycle so the statistics always reflect the latest wallet set.

## Target Composition for 71 Bits
1. Regression baseline - Regress each run-length share against wallet index (1-70) and project to index 71. The table in analytics/wallet71_prediction_algorithm.md already lists the projected shares and expected run counts (for example, 1x1 share 0.094, translating to about 6.65 runs over 71 bits).
2. Momentum injection - Extract the latest share vector s70 and the most recent change d70 from the ratio files. Blend them with the regression projection:
   ```
   s_target = normalize(alpha * s_reg + (1 - alpha) * (s70 + beta * d70))
   ```
   Recommended defaults: alpha = 0.6, beta = 1.0. Clamp negative interim shares to zero before the final renormalisation.
3. Expected run counts - Convert s_target into run-count expectations with `expected_runs[r] = s_target[r] * 71 / run_length(r)`. The forecast points to roughly eight 1x1 runs, four 1x2 runs, two 1x3 runs, eight 0x1 runs, four 0x2 runs, and elevated chances (60-80%) of observing at least one 0x4 or 0x5 block (bits_statistic.md lines 15-102).

## Run-Budget Sampling
1. Treat expected_runs as the mean of a Dirichlet prior shaped by the historical frequency of each run length (51.6% of one runs are length 1, 52.6% of zero runs are length 1, etc.).
2. Sample a concrete run budget c[r] from the Dirichlet-multinomial while enforcing alternating feasibility: the total number of 1-runs must differ from zero-runs by at most one so the sequence can start with a 1 block.
3. Draw the total run count from a truncated normal (mu = 35, sigma = 5) constrained so the implied bit total lands within 71 +/- 2 before any final-run trimming. Historical sequences cluster between 18 and 30 runs with a median of 19 (bits_statistic.md lines 76-78).

## Sequence Construction
1. Initial run - Sample the opener from the empirical start distribution (1x1 48.6%, 1x2 28.6%, 1x3 12.9%, etc.), weighted by the remaining budget (`weight = start_prob * (c[r] + eps)^eta`, eta about 0.8). Force the first block to be a 1-run.
2. Iterative steps - Alternate between ones and zeros. For each candidate next run, compute
   ```
   score(next) = P(prev -> next)^gamma * (c[next] + eps)^eta * balance_penalty(next)
   ```
   - Transition likelihoods come from the historical matrix (for example, 1x1 -> 0x1: 26.8%, 0x1 -> 1x1: 28.6%).
   - gamma about 1.0 preserves empirical shape; reduce toward 0.8 for more exploration.
   - balance_penalty down-weights moves that would exhaust the remaining budget or overshoot the 71-bit ceiling.
3. Length guardrail - Track cumulative bits and stop once 71 bits are reached. If the last run would overflow, truncate that run to fit exactly 71 bits and record the adjustment for scoring.
4. Fallbacks - If a needed run category is exhausted, fall back to the global bit-share prior for that bit type. Penalise the candidate later so the search prefers budgets that remained feasible.

## Candidate Scoring and Selection
1. Generate N candidates (typically 200) via stochastic DFS or beam search limited to the top k transitions per step (k about 5). These limits keep exploration manageable relative to the ~1.0e21 alternating run combinations available at 71 bits (analytics/wallet71_combination_volume.md lines 10-44).
2. For each completed sequence, recompute realised run shares and compare to s_target using a weighted L1 distance. Add penalties for:
   - Run counts deviating by more than one from the sampled budget.
   - Transition fallbacks or truncated end runs.
   - Duplicate run histograms or transition log-likelihoods already seen (novelty enforcement).
3. Rank candidates by total score and keep the best (optionally the top-K list). Persist each winner with its (bit, length) run list, realised shares, penalty breakdown, and the hyperparameters used to create it.

## Validation Checklist
- Re-run the analytics pipeline on the selected sequence to confirm the realised run counts fall inside the historical interquartile ranges highlighted in analytics/bits_statistic.md.
- Verify the opening trio of runs aligns with the dominant motifs (1x1-0x1-1x1 or 1x2-0x1-1x1) unless the scoring routine rewarded intentional deviations.
- Compute the log-likelihood of the run sequence under the transition matrix to flag outliers whose behaviour is too improbable versus history.
- Log RNG seeds and parameter values (alpha, beta, gamma, eta, N, k) so results are reproducible.

Adhering to these steps yields wallet candidates that respect both the long-term run-length hierarchy and the recent shifts called out in the 71st-bit statistical forecast.
