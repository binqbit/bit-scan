# Wallet 71 Generation Usage & Implementation Plan

## Purpose
This document explains how to operationalise the statistical framework described in `analytics/wallet71_prediction_algorithm.md` for producing candidate 71-bit wallets. It defines how probabilities are assembled, how "smart randomness" is applied, and how to enumerate without wasting effort on permutations that do not change observable behaviour.

## Inputs & Preprocessing
- Recompute run statistics from the source artefacts before every generation cycle to keep the model aligned with the latest 70 wallets.
- Cache the following structures in memory:
  - Target share vector `s_target` (blended regression + momentum).
  - Expected run counts per category (`1x1`, `0x1`, etc.) scaled to 71 bits.
  - Start-of-line distribution and terminal distribution.
  - Transition matrix `P(prev -> next)` with Laplace smoothing.
  - Global run-length priors for ones and zeros (used as pseudocounts).
- Enforce the domain constraints at load time: sequences start with a `1` run, alternate bit type, and total trimmed length must stay within 71 bits.

## Probability Model Assembly
1. **Target Share Blend** - Combine regression projection and recent momentum with weights `(alpha, beta)` (default `alpha = 0.6`, `beta = 1.0`). Clamp negative values, then renormalise.
2. **Run Budget Prior** - Interpret `expected_runs` as the mean of a Dirichlet prior. Add pseudocounts proportional to historical frequencies so rare categories stay available but low probability.
3. **Transition Weights** - Raise transition probabilities to power `gamma` (default `gamma = 1.0`). Lower `gamma` (< 1) flattens to encourage exploration; higher (> 1) emphasises historical tendencies.
4. **Length Guardrails** - Maintain cumulative bit length while sampling. Hard-stop once 71 bits is reached; truncate only the final run if needed.

## Smart Random Generation Pipeline
1. **Initial Run Selection**
   - Use start distribution scaled by remaining run budget (`w(r) = start_prob(r) * (c[r] + eps)^eta`).
   - Enforce starting with a `1` run (per dataset reality). Reject any zero-start sample.
2. **Iterative Run Sampling**
   - At each step, compute candidate scores: `score(next) = P(prev -> next)^gamma * (c[next] + eps)^eta * balance_penalty(next)`.
   - `balance_penalty` evaluates whether choosing `next` leaves enough room for the remaining expected runs without breaking the 71-bit cap.
3. **Uniqueness Enforcement**
   - Maintain canonical representation of the run sequence (e.g., tuple of `(bit, length)` pairs). Hash to detect duplicates before acceptance.
   - Reject moves that simply swap two consecutive runs of equal bit/length order; they produce identical bit strings.
   - Apply a novelty score that penalises any candidate whose run histogram matches a previously accepted sequence and whose transition log-likelihood differs by < epsilon.
4. **Backtracking**
   - If no valid next run fits the remaining budget and length, backtrack to the previous step and resample with the depleted category removed.
5. **Termination**
   - When cumulative length hits 71, emit the sequence. If the last run overflows, trim its length and record the deficit in the scoring metadata.

## Avoiding Redundant Permutations
- Treat the run sequence as strictly alternating; swapping adjacent runs of the same bit type is impossible by construction, eliminating redundant rearrangements.
- Normalise consecutive equal-length runs of alternating bits into canonical order (lexicographic on run length) during deduplication to avoid mirrored patterns.
- Keep a trie of partial run prefixes. If a prefix leads to the same residual budget and length window as an earlier explored branch, reuse its outcome rather than resampling.

## Enumeration Strategy
- Implement a probability-guided depth-first search (DFS) that branches on the top-`k` probable next runs (`k` configurable, default 5).
- Use upper-bound scoring (sum of remaining expected shares) to prune branches whose best-case score already exceeds the worst accepted candidate.
- Cache evaluated states `(index, prev_run, remaining_budget, remaining_bits)` to avoid recomputation.
- Produce outputs in ranked order (lowest score first), enabling downstream systems to consume the best `N` unique sequences.

## Integration of Zero-Run Probabilities
- Zero-run categories follow the same budgeting rules as ones. The generator always inserts at least one zero bit between `1` runs because runs alternate by definition.
- For combined probabilities (e.g., `1x2` followed by `0x3`), rely on the transition matrix. The probability of a composite pattern equals the product of edge probabilities along the run path.
- To score longer motifs (e.g., `1x2`-`0x3`-`1x1`), precompute n-gram likelihoods from historical data and blend them with the run-by-run score when assessing uniqueness.

## Implementation Roadmap
1. **Data Loader Module** - Parse CSV/TXT inputs, rebuild run statistics, compute `s_target`.
2. **Probability Engine** - Encapsulate Dirichlet sampling, transition scoring, and start/end distributions.
3. **Run Budget Sampler** - Sample integer counts that satisfy alternating-run parity and 71-bit feasibility checks.
4. **Sequence Builder** - Implement smart random walk with backtracking, uniqueness hashing, and length guards.
5. **Enumerator** - Layer DFS/beam search on top of the builder to exhaustively explore high-probability sequences without duplication.
6. **Scoring & Ranking** - Calculate deviation from `s_target`, log-likelihood, novelty score, and final composite score.
7. **CLI/Service Wrapper** - Offer commands for (a) single smart-random sample, (b) top-`N` enumeration, (c) diagnostic reports.
8. **Testing & Backtests** - Replay the pipeline on historical wallets 40-70 to validate distribution fidelity and dedupe logic.

## Usage Modes
- **Smart Random Sampling** - Generate diverse yet statistically grounded candidates by running the pipeline with stochastic sampling enabled and dedupe filters active.
- **Guided Enumeration** - Systematically walk the search space, stopping after `N` unique high-scoring sequences. Suitable for exhaustive review or brute-force validation.
- **Hybrid Mode** - Begin with enumeration until marginal gain drops, then switch to stochastic sampling with heightened novelty penalties to discover outliers.

## Operational Checklist
- Verify input data freshness and recompute statistics.
- Calibrate `(alpha, beta, gamma, eta)` hyperparameters using rolling backtests.
- Seed RNG for reproducibility but allow overrides.
- Persist generated sequences with metadata: run list, scores, hash keys used for dedupe, and the exact parameter set.
- After publishing wallet 71, append to datasets and retrain models before re-running for wallet 72.
