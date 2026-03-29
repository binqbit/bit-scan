# Analytics Overview

## Source Data
- [private_keys_1_70_bit.csv](./private_keys_1_70_bit.csv) - Master list of 70 wallets with their full bit strings plus hex and decimal forms.

## Run-Length Frequency Tables
- [1_bits.txt](./1_bits.txt) - For each wallet (`line` column), lists how many times a contiguous run of `1`s appears at every observed length. Each column header is a run length and the entries show counts for that wallet.
- [0_bits.txt](./0_bits.txt) - Mirrors the `1` table but for runs of `0`s after the leading zeros are removed.

## Run-Length Share Tables
- [bit_block_ratios.txt](./bit_block_ratios.txt) - Shows, per wallet, how the trimmed bit string is divided among the various run lengths. The row values sum to 1.0, expressing each run length as a share of the whole line.
- [bit_block_ratio_changes.txt](./bit_block_ratio_changes.txt) - Highlights how those shares shift from one wallet to the next. Row *n* records the change in share relative to wallet *n-1* (row 1 reflects its move from an all-zero baseline).

Use these references to understand how consecutive `1` and `0` blocks are distributed, how they evolve across wallets, and how one block length tends to follow another.
