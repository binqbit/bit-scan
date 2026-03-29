# 71st Wallet Bit Block Forecast

## Data and Method
- Source tables: analytics/private_keys_1_70_bit.csv, analytics/1_bits.txt, analytics/0_bits.txt, analytics/bit_block_ratios.txt, and analytics/bit_block_ratio_changes.txt.
- Trimmed each wallet's bit string (leading zeros removed) and rebuilt the exact run-length sequences for all 70 wallets.
- Computed empirical count distributions, expected values, and modal outcomes for every observed run length of ones and zeros; repeated the same calculations on the latest 10 and 5 wallets to capture momentum.
- Derived ordering statistics from the reconstructed sequences: first-run frequencies, early-run patterns, and top transition pairs between consecutive runs.
- Summaries below present the likelihood that the 71st wallet repeats the historical behaviour under a stationary assumption, with recent windows highlighting short-term shifts.

## Block Count Probabilities

### Runs of 1s
| block | P(>=1) all | Exp count all | Mode (prob) | P(>=1) last10 | Exp last10 | P(>=1) last5 | Exp last5 |
|-------|------------|---------------|-------------|---------------|------------|--------------|-----------|
| 1x1 | 95.7% | 4.90 | 6 (14.3%) | 100% | 7.90 | 100% | 7.60 |
| 1x2 | 82.9% | 2.47 | 1 (21.4%) | 100% | 4.70 | 100% | 4.80 |
| 1x3 | 67.1% | 1.09 | 1 (37.1%) | 100% | 1.80 | 100% | 2.40 |
| 1x4 | 40.0% | 0.56 | 0 (60.0%) | 70% | 1.30 | 60% | 1.00 |
| 1x5 | 25.7% | 0.27 | 0 (74.3%) | 40% | 0.40 | 20% | 0.20 |
| 1x6 | 15.7% | 0.19 | 0 (84.3%) | 40% | 0.50 | 60% | 0.80 |
| 1x7 | 1.4% | 0.01 | 0 (98.6%) | 0% | 0.00 | 0% | 0.00 |
| 1x10 | 1.4% | 0.01 | 0 (98.6%) | 0% | 0.00 | 0% | 0.00 |

Key takeaways:
- Single-bit runs dominate: the next wallet almost surely contains between 6 and 8 short 1x1 blocks, higher than the long-run average.
- Double and triple runs are also near-certain in recent windows, roughly doubling their historical expected counts.
- Longer runs (length >=5) remain rare overall but have appeared more often in the latest wallets, especially 1x6 bursts.

### Runs of 0s
| block | P(>=1) all | Exp count all | Mode (prob) | P(>=1) last10 | Exp last10 | P(>=1) last5 | Exp last5 |
|-------|------------|---------------|-------------|---------------|------------|--------------|-----------|
| 0x1 | 88.6% | 4.74 | 7 (14.3%) | 100% | 8.30 | 100% | 8.20 |
| 0x2 | 80.0% | 2.20 | 2 (24.3%) | 90% | 4.00 | 100% | 4.00 |
| 0x3 | 54.3% | 0.96 | 0 (45.7%) | 100% | 1.60 | 100% | 1.40 |
| 0x4 | 38.6% | 0.56 | 0 (61.4%) | 70% | 1.00 | 60% | 1.20 |
| 0x5 | 27.1% | 0.34 | 0 (72.9%) | 80% | 1.10 | 80% | 1.20 |
| 0x6 | 5.7% | 0.06 | 0 (94.3%) | 10% | 0.10 | 20% | 0.20 |
| 0x7 | 11.4% | 0.11 | 0 (88.6%) | 20% | 0.20 | 20% | 0.20 |
| 0x8 | 2.9% | 0.03 | 0 (97.1%) | 0% | 0.00 | 0% | 0.00 |
| 0x9 | 1.4% | 0.01 | 0 (98.6%) | 0% | 0.00 | 0% | 0.00 |

Key takeaways:
- Short 0x1 and 0x2 runs remain ubiquitous; recent wallets double the historical expectations, pointing to denser zero fragmentation.
- Mid-length 0 runs (0x4-0x5) are ramping up quickly, with 70-80% recent occurrence rates versus sub-30% all-time probabilities.
- Very long 0 runs (0x8+) are still statistical outliers.

## Sequence Ordering Probabilities

### First-run and Early Patterns
| first run | probability |
|-----------|-------------|
| 1x1 | 48.6% |
| 1x2 | 28.6% |
| 1x3 | 12.9% |
| 1x4 | 4.3% |
| 1x5 | 2.9% |
| 1x6 | 2.9% |

Top run pairs (initial 1 block followed by the first 0 block):
- 1x1 -> 0x1 with 27.1% likelihood.
- 1x2 -> 0x1 with 21.4%.
- 1x1 -> 0x2 with 11.4%.
- 1x3 -> 0x1 with 7.1%.
- 1x1 -> 0x3 with 5.7%.

Top three-block openings (1, 0, 1 runs):
- 1x1, 0x1, 1x1 and 1x2, 0x1, 1x1 each occur 14.3% of the time.
- 1x1, 0x1, 1x3 follows in 7.1% of wallets.
- 1x3, 0x1, 1x1 and 1x1, 0x2, 1x1 each appear about 5.7%.

### Transition Likelihoods Throughout the Sequence
- 1 runs most often hand off to short 0 runs: 1x1 -> 0x1 (26.8%), 1x2 -> 0x1 (13.8%), and 1x1 -> 0x2 (13.5%).
- The inverse transitions mirror this behaviour: 0x1 -> 1x1 (28.6%), 0x1 -> 1x2 (14.0%), 0x2 -> 1x1 (12.3%).
- Cross transitions into longer blocks, such as 1x1 -> 0x4 or 0x2 -> 1x4, remain below 3% each, but their frequency has grown in the latest runs.

### Run Counts and Sequence Lengths
- Median sequence length is 19 runs (min 1, max 38); 41.4% of wallets fall between 18 and 30 runs.
- Average per wallet: 9.50 one-runs and 9.01 zero-runs, with 48.6% of wallets containing more one runs than zero runs.

## Block Share Trends (bit-level weight)
| block | Overall share | Last 10 share | Last 5 share | Latest wallet | Latest change |
|-------|---------------|---------------|--------------|---------------|---------------|
| 1x1 | 15.93% | 12.11% | 11.19% | 11.43% | +4.18 pts |
| 1x2 | 14.89% | 14.32% | 14.06% | 22.86% | +11.26 pts |
| 1x3 | 9.77% | 8.16% | 10.63% | 8.57% | -0.12 pts |
| 1x4 | 5.60% | 8.09% | 5.94% | 5.71% | -0.08 pts |
| 1x6 | 2.30% | 4.47% | 7.03% | 0.00% | -17.39 pts |
| 0x1 | 12.47% | 12.75% | 12.13% | 10.00% | +1.30 pts |
| 0x2 | 12.42% | 12.20% | 11.70% | 17.14% | +5.55 pts |
| 0x3 | 8.33% | 7.33% | 6.13% | 12.86% | +8.51 pts |
| 0x4 | 5.15% | 6.06% | 7.04% | 11.43% | +11.43 pts |
| 0x5 | 4.90% | 8.38% | 8.87% | 0.00% | -14.49 pts |
| 0x7 | 2.68% | 2.13% | 2.03% | 0.00% | -10.14 pts |

Interpretation:
- The latest wallet leans heavily on 1x2 and 0x4 blocks, reversing earlier surges in very long runs.
- Recent growth in 0x3-0x5 shares aligns with the higher probabilities of multiple medium-length zero runs shown above.

## Forecast for Wallet 71
- Expect dense clusters of short runs: roughly 8 single-bit 1 blocks and 8 single-bit 0 blocks, with near-certain follow-up 1x2, 1x3, 0x2, and 0x3 activity.
- Medium-length zero blocks (0x4-0x5) now carry a 60-80% chance of appearing at least once, whereas longer one blocks (>=5) remain less than even odds despite recent spikes.
- The opening of the sequence is most likely to mirror historic leaders (1x1 -> 0x1 -> 1x1 or 1x2 -> 0x1 -> 1x1), and subsequent transitions should continue the alternating chain of short runs with occasional medium-length insertions.

## Conditional Interaction Highlights

### Opening Block Influence
Overall next zero-run mix after any 1-run: 0x1 52.6%, 0x2 24.4%, 0x3 10.6%, 0x4+ 12.4%.

| first 1-block | next 0x1 | next 0x2 | next 0x3 | next 0x4+ | sample |
|---------------|----------|----------|----------|-----------|--------|
| 1x1 | 57.6% (+5.0 pts) | 24.2% (-0.2 pts) | 12.1% (+1.5 pts) | 6.1% (-6.3 pts) | 33 |
| 1x2 | 78.9% (+26.3 pts) | 5.3% (-19.1 pts) | 5.3% (-5.3 pts) | 10.5% (-1.9 pts) | 19 |
| 1x3 | 62.5% (+9.9 pts) | 12.5% (-11.9 pts) | 0.0% (-10.6 pts) | 25.0% (+12.6 pts) | 8 |
| 1x4+ | 71.4% (+18.8 pts) | 14.3% (-10.1 pts) | 0.0% (-10.6 pts) | 14.3% (+1.9 pts) | 7 |

Key effects:
- Opening with 1x2 locks the second block into a 0x1 run almost eight times out of ten while suppressing 0x2 repeats, signalling a rapid flip-flop pattern.
- First-run lengths of 1x3 or longer more than double the odds of an immediate long zero (0x4+) versus the baseline, setting up early deep zero troughs.
- Wallets starting with 1x1 curb the very next zero run but still host 0x5 blocks later in the line 32% of the time (vs. 27% overall), whereas 1x2 openers cut that tail to 10%.

### Zero Mix Given the Opener
Probability of seeing key zero block types somewhere in the same bit string:

| first 1-block | P(0x1) | P(0x2) | P(0x3) | P(0x4) | P(0x5) |
|---------------|--------|--------|--------|--------|--------|
| 1x1 | 88.2% | 79.4% | 55.9% | 35.3% | 32.4% |
| 1x2 | 90.0% | 75.0% | 60.0% | 40.0% | 10.0% |
| 1x3 | 77.8% | 77.8% | 44.4% | 44.4% | 33.3% |

(Overall probabilities: 0x1 88.6%, 0x2 80.0%, 0x3 54.3%, 0x4 38.6%, 0x5 27.1%.) The 1x2 opener trims the heavy tail of long zero runs, while 1x3 boosts both 0x4 and 0x5 incidence above the norm.

### Zero-to-One Feedback
Baseline next 1-run mix after any zero: 1x1 51.9%, 1x2 25.7%, 1x3 11.3%, 1x4+ 11.1%.

| previous 0-block | next 1x1 | next 1x2 | next 1x3 | next 1x4+ | sample |
|------------------|----------|----------|----------|-----------|--------|
| 0x1 | 53.3% (+1.4 pts) | 26.0% (+0.3 pts) | 12.2% (+0.9 pts) | 8.5% (-2.6 pts) | 319 |
| 0x2 | 52.1% (+0.2 pts) | 25.7% (+0.0 pts) | 10.0% (-1.3 pts) | 12.1% (+1.0 pts) | 140 |
| 0x4 | 50.0% (-1.9 pts) | 22.2% (-3.5 pts) | 11.1% (-0.2 pts) | 16.7% (+5.6 pts) | 36 |
| 0x5 | 36.4% (-15.5 pts) | 40.9% (+15.2 pts) | 13.6% (+2.3 pts) | 9.1% (-2.0 pts) | 22 |
| 0x7+ | 46.7% (-5.2 pts) | 20.0% (-5.7 pts) | 6.7% (-4.6 pts) | 26.7% (+15.6 pts) | 15 |

Aggregating zeros into short (<=2) versus long (>=4) confirms the regime change: after a short zero block the next one block is 1x1 in 53.0% of cases, while after a long zero it drops to 45.2% and 1x4+ jumps to 13.7% (vs. 7.8% when coming off a short block).
