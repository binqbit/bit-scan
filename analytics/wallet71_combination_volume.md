# Wallet 71 Combination Volume Analysis

## Assumptions
- Start with a 1-run and alternate bit type; matches the historical dataset and the scan_v2 builder.
- Allowed run lengths mirror the generator categories: 1x{1,2,3,4,5,6,7,10} for ones and 0x{1,2,3,4,5,6,7,8,9} for zeros.
- A run may terminate early in the final position (the generator trims the last block to fit the requested bit budget), so any category whose declared length exceeds the remaining bits contributes one finishing path.
- Counts represent unique bit strings; metadata about which oversized category produced the truncated ending is ignored because the emitted bits are identical.

## Raw Sequence Counts vs Bit Length
Dynamic programming over alternating runs yields the number of admissible sequences for several target lengths. A 71-bit target already implies more than 1e21 distinct patterns.

| target bits | combinations |
| ----------- | ------------ |
| 60 | 5.12e17 |
| 65 | 1.62e19 |
| 70 | 5.13e20 |
| **71** | **1.02e21** |
| 75 | 1.62e22 |
| 80 | 5.14e23 |
| 90 | 5.14e26 |

For comparison, the entire 71-bit binary space contains 2^71 ≈ 2.36e21 elements. The alternating-run constraints therefore prune only about 57% of the naive space; 43.3% of all 71-bit strings remain valid under the pattern rules. This ceiling intentionally ignores the pragmatic filters we layer on top (Dirichlet budgets, transition weighting, novelty hashing, capped node budgets, etc.).

## Effect of Run-Count Budgeting
scan_v2 samples total runs from a normal prior centered near the blended expectation (~35). Imposing a hard cap on the number of runs dramatically shrinks the search space, but the volume is still astronomical if we allow every combination that satisfies the cap.

| max runs allowed | combinations | share of unconstrained space |
| ---------------- | ------------ | ---------------------------- |
| 30 | 6.99e19 | 6.8% |
| 35 | 4.18e20 | 40.8% |
| 40 | 8.61e20 | 84.2% |
| 45 | 1.01e21 | 98.7% |
| 50 | 1.02e21 | 99.97% |

(Each combination count assumes that finishing the sequence consumes one run, so the ceiling is applied before the last block is trimmed.)

## Heuristic-Pruned Search Volume
- **Run-budget sampling** – Keeping each category within roughly ±1 of its target run count collapses the combinatorial surface by well over two orders of magnitude. Monte-Carlo enumeration with those bounds lands at ≈8e18 unique sequences: >120× smaller than the unconstrained alternating set and already ~300× below the naïve 2^71 sweep.
- **Transition weighting** – Restricting each step to the top-k historical transitions (k≈5) while enforcing the 71-bit guardrails leaves ≈3e17 viable branches, a further ~30× contraction. We are now ~4000× tighter than the alternating ceiling.
- **Searcher limits** – The DFS respects `max_nodes = 8000` and `max_attempts = 64`. In practice we visit ≤8e3 partial states per attempt and accept only novel completions, so real exploration for one key stays in the O(10^4) range—about 10^17× smaller than 2^71.

These stacked optimisations give us well over a 100× reduction in practice (often many orders beyond that); the theoretical counts above simply highlight how quickly things explode once those guardrails are relaxed.

## Implications for Enumeration
- Even with statistical targeting, the potential state space is immense—full enumeration is impractical beyond very tight caps (< 30 runs) or aggressive probabilistic pruning (top-k branching, novelty scoring, etc.).
- Truncation of the final run provides only limited relief; the branching factor for mid-sequence steps dominates the explosion.
- Any "too much" scenario—e.g., letting the DFS explore deep without pruning—quickly approaches the same cardinality as the entire 71-bit space. This is why the implementation enforces capped node budgets, Dirichlet-guided run counts, and novelty hashing to keep the search feasible.

These counts should guide parameter tuning: reduce max_nodes, tighten run-budget variance, or raise novelty penalties whenever empirical exploration threatens to diverge toward the theoretical maxima shown above.
