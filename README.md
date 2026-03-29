# Bit Scan

A simple utility for searching Bitcoin wallets [Bitcoin Puzzle Transaction](https://bitcointalk.org/index.php?topic=1306983.0)

## Usage

```bash
bit-scan scan [--version <v1|v2|v3|v4>] [--stats] [--threads <count>] <target>
bit-scan check <address> <private_key>
```

- `scan` searches for the private key that matches the given Base58 Bitcoin address.
- `--version` picks the scanning engine (`v1` brute force, `v2` pattern-guided, `v3` CUDA batch, `v4` multi-threaded CPU).
- `--threads` sets the worker pool size (required when `--version v4` is selected).
- `--stats` prints a rolling throughput report once per second (candidates per second plus cumulative total).
- `target` can be either a Bitcoin address **or** the puzzle number listed in the table below.
- Bit length is inferred from the target (puzzle numbers supply their own size).
- `private_key` for `check` accepts up to 64 hex characters (optionally prefixed with `0x`); shorter inputs are left-padded with zeros.

## Example

Run a quick scan against puzzle wallet 10 using the pattern-driven engine:

```bash
bit-scan scan --stats 10
```

Run the multi-threaded CPU engine on puzzle wallet 10 with eight workers:

```bash
bit-scan scan --version v4 --threads 8 --stats 10
```

If you prefer to scan by address, add an entry to `config/puzzle_addresses.csv` so the tool can infer the correct bit length.

Validate a private key against the same wallet (supports optional `0x` prefix):

```bash
bit-scan check 1LeBZP5QCwwgXRtmVUvTVrraqPUokyLHqe 000000000000000202
```

### CUDA engine (v3)

The `v3` engine batches candidate generation on the GPU via CUDA. Enable it at build time with:

```bash
cargo build --release --features cuda
```

Requirements:
- NVIDIA GPU with a recent driver.
- CUDA toolkit installed (`CUDA_PATH`, `CUDA_ROOT`, or `CUDA_TOOLKIT_ROOT_DIR` must point to it).

Runtime tuning knobs for `v3`:
- `BIT_SCAN_V3_BATCH_SIZE` sets candidates per GPU batch. Larger batches use more VRAM and RAM.
- `BIT_SCAN_V3_VERIFY_THREADS` sets how many CPU threads verify generated candidates.
- `BIT_SCAN_V3_BLOCK_SIZE` sets CUDA threads per block.
- `BIT_SCAN_V3_ITEMS_PER_THREAD` sets how many candidates each CUDA thread generates before exiting.

Example aggressive configuration:

```bash
BIT_SCAN_V3_BATCH_SIZE=2097152 \
BIT_SCAN_V3_VERIFY_THREADS=15 \
BIT_SCAN_V3_ITEMS_PER_THREAD=128 \
cargo run --release -- scan --version v3 --stats 71
```

If CUDA support is missing at build or runtime, version 3 automatically falls back to the multi-threaded CPU (`v4`) implementation.

## Found wallets

| №   | Wallet Address                     | Private Key (hex)  | Status |
| --- | ---------------------------------- | ------------------ | ------ |
| 1   | 1BgGZ9tcN4rm9KBzDn7KprQz87SZ26SAMH | 000000000000000001 | Solved |
| 2   | 1CUNEBjYrCn2y1SdiUMohaKUi4wpP326Lb | 000000000000000003 | Solved |
| 3   | 19ZewH8Kk1PDbSNdJ97FP4EiCjTRaZMZQA | 000000000000000007 | Solved |
| 4   | 1EhqbyUMvvs7BfL8goY6qcPbD6YKfPqb7e | 000000000000000008 | Solved |
| 5   | 1E6NuFjCi27W5zoXg8TRdcSRq84zJeBW3k | 000000000000000015 | Solved |
| 6   | 1PitScNLyp2HCygzadCh7FveTnfmpPbfp8 | 000000000000000031 | Solved |
| 7   | 1McVt1vMtCC7yn5b9wgX1833yCcLXzueeC | 00000000000000004c | Solved |
| 8   | 1M92tSqNmQLYw33fuBvjmeadirh1ysMBxK | 0000000000000000e0 | Solved |
| 9   | 1CQFwcjw1dwhtkVWBttNLDtqL7ivBonGPV | 0000000000000001d3 | Solved |
| 10  | 1LeBZP5QCwwgXRtmVUvTVrraqPUokyLHqe | 000000000000000202 | Solved |
| 11  | 1PgQVLmst3Z314JrQn5TNiys8Hc38TcXJu | 000000000000000483 | Solved |
| 12  | 1DBaumZxUkM4qMQRt2LVWyFJq5kDtSZQot | 000000000000000a7b | Solved |
| 13  | 1Pie8JkxBT6MGPz9Nvi3fsPkr2D8q3GBc1 | 000000000000001460 | Solved |
| 14  | 1ErZWg5cFCe4Vw5BzgfzB74VNLaXEiEkhk | 000000000000002930 | Solved |
| 15  | 1QCbW9HWnwQWiQqVo5exhAnmfqKRrCRsvW | 0000000000000068f3 | Solved |
| 16  | 1BDyrQ6WoF8VN3g9SAS1iKZcPzFfnDVieY | 00000000000000c936 | Solved |
| 17  | 1HduPEXZRdG26SUT5Yk83mLkPyjnZuJ7Bm | 00000000000001764f | Solved |
| 18  | 1GnNTmTVLZiqQfLbAdp9DVdicEnB5GoERE | 00000000000003080d | Solved |
| 19  | 1NWmZRpHH4XSPwsW6dsS3nrNWfL1yrJj4w | 00000000000005749f | Solved |
| 20  | 1HsMJxNiV7TLxmoF6uJNkydxPFDog4NQum | 0000000000000d2c55 | Solved |
| 21  | 14oFNXucftsHiUMY8uctg6N487riuyXs4h | 0000000000001ba534 | Solved |
| 22  | 1CfZWK1QTQE3eS9qn61dQjV89KDjZzfNcv | 0000000000002de40f | Solved |
| 23  | 1L2GM8eE7mJWLdo3HZS6su1832NX2txaac | 000000000000556e52 | Solved |
| 24  | 1rSnXMr63jdCuegJFuidJqWxUPV7AtUf7  | 000000000000dc2a04 | Solved |
| 25  | 15JhYXn6Mx3oF4Y7PcTAv2wVVAuCFFQNiP | 000000000001fa5ee5 | Solved |
| 26  | 1JVnST957hGztonaWK6FougdtjxzHzRMMg | 00000000000340326e | Solved |
| 27  | 128z5d7nN7PkCuX5qoA4Ys6pmxUYnEy86k | 000000000006ac3875 | Solved |
| 28  | 12jbtzBb54r97TCwW3G1gCFoumpckRAPdY | 00000000000d916ce8 | Solved |
| 29  | 19EEC52krRUK1RkUAEZmQdjTyHT7Gp1TYT | 000000000017e2551e | Solved |
| 30  | 1LHtnpd8nU5VHEMkG2TMYYNUjjLc992bps | 00000000003d94cd64 | Solved |
| 31  | 1LhE6sCTuGae42Axu1L1ZB7L96yi9irEBE | 00000000007d4fe747 | Solved |
| 32  | 1FRoHA9xewq7DjrZ1psWJVeTer8gHRqEvR | 0000000000b862a62e | Solved |
| 33  | 187swFMjz1G54ycVU56B7jZFHFTNVQFDiu | 0000000001a96ca8d8 | Solved |
| 34  | 1PWABE7oUahG2AFFQhhvViQovnCr4rEv7Q | 00000000034a65911d | Solved |
| 35  | 1PWCx5fovoEaoBowAvF5k91m2Xat9bMgwb | 0000000004aed21170 | Solved |
| 36  | 1Be2UF9NLfyLFbtm3TCbmuocc9N1Kduci1 | 0000000009de820a7c | Solved |
| 37  | 14iXhn8bGajVWegZHJ18vJLHhntcpL4dex | 000000001757756a93 | Solved |
| 38  | 1HBtApAFA9B2YZw3G2YKSMCtb3dVnjuNe2 | 0000000022382facd0 | Solved |
| 39  | 122AJhKLEfkFBaGAd84pLp1kfE7xK3GdT8 | 000000004b5f8303e9 | Solved |
| 40  | 1EeAxcprB2PpCnr34VfZdFrkUWuxyiNEFv | 00000000e9ae4933d6 | Solved |
| 41  | 1L5sU9qvJeuwQUdt4y1eiLmquFxKjtHr3E | 0000000153869acc5b | Solved |
| 42  | 1E32GPWgDyeyQac4aJxm9HVoLrrEYPnM4N | 00000002a221c58d8f | Solved |
| 43  | 1PiFuqGpG8yGM5v6rNHWS3TjsG6awgEGA1 | 00000006bd3b27c591 | Solved |
| 44  | 1CkR2uS7LmFwc3T2jV8C1BhWb5mQaoxedF | 0000000e02b35a358f | Solved |
| 45  | 1NtiLNGegHWE3Mp9g2JPkgx6wUg4TW7bbk | 000000122fca143c05 | Solved |
| 46  | 1F3JRMWudBaj48EhwcHDdpeuy2jwACNxjP | 0000002ec18388d544 | Solved |
| 47  | 1Pd8VvT49sHKsmqrQiP61RsVwmXCZ6ay7Z | 0000006cd610b53cba | Solved |
| 48  | 1DFYhaB2J9q1LLZJWKTnscPWos9VBqDHzv | 000000ade6d7ce3b9b | Solved |
| 49  | 12CiUhYVTTH33w3SPUBqcpMoqnApAV4WCF | 00000174176b015f4d | Solved |
| 50  | 1MEzite4ReNuWaL5Ds17ePKt2dCxWEofwk | 0000022bd43c2e9354 | Solved |
| 51  | 1NpnQyZ7x24ud82b7WiRNvPm6N8bqGQnaS | 0000075070a1a009d4 | Solved |
| 52  | 15z9c9sVpu6fwNiK7dMAFgMYSK4GqsGZim | 00000efae164cb9e3c | Solved |
| 53  | 15K1YKJMiJ4fpesTVUcByoz334rHmknxmT | 0000180788e47e326c | Solved |
| 54  | 1KYUv7nSvXx4642TKeuC2SNdTk326uUpFy | 0000236fb6d5ad1f43 | Solved |
| 55  | 1LzhS3k3e9Ub8i2W1V8xQFdB8n2MYCHPCa | 00006abe1f9b67e114 | Solved |
| 56  | 17aPYR1m6pVAacXg1PTDDU7XafvK1dxvhi | 00009d18b63ac4ffdf | Solved |
| 57  | 15c9mPGLku1HuW9LRtBf4jcHVpBUt8txKz | 0001eb25c90795d61c | Solved |
| 58  | 1Dn8NF8qDyyfHMktmuoQLGyjWmZXgvosXf | 0002c675b852189a21 | Solved |
| 59  | 1HAX2n9Uruu9YDt4cqRgYcvtGvZj1rbUyt | 0007496cbb87cab44f | Solved |
| 60  | 1Kn5h2qpgw9mWE5jKpk8PP4qvvJ1QVy8su | 000fc07a1825367bbe | Solved |
| 61  | 1AVJKwzs9AskraJLGHAZPiaZcrpDr1U6AB | 0013c96a3742f64906 | Solved |
| 62  | 1Me6EfpwZK5kQziBwBfvLiHjaPGxCKLoJi | 00363d541eb611abee | Solved |
| 63  | 1NpYjtLira16LfGbGwZJ5JbDPh3ai9bjf4 | 007cce5efdaccf6808 | Solved |
| 64  | 16jY7qLJnxb7CHZyqBP8qca9d51gAjyXQN | 00f7051f27b09112d4 | Solved |
| 65  | 18ZMbwUFLMHoZBbfpCjUJQTCMCbktshgpe | 01a838b13505b26867 | Solved |
| 66  | 13zb1hQbWVsc2S7ZTZnP2G4undNNpdh5so | 02832ed74f2b5e35ee | Solved |
| 67  | 1BY8GQbnueYofwSuFAT3USAhGjPrkxDdW9 | 0730fc235c1942c1ae | Solved |
| 68  | 1MVDYgVaSN6iKKEsbzRUAYFrYJadLYZvvZ | 0bebb3940cd0fc1491 | Solved |
| 69  | 19vkiEajfhuZ8bs8Zu2jgmC6oqZbWqhxhG | 101d83275fb2bc7e0c | Solved |
| 70  | 19YZECXj3SxEZMoUeJ1yiPsw8xANe7M7QR | 349b84b6431a6c4ef1 | Solved |

## Not solved yet

| №   | Wallet Address                      | Private Key (hex)                | Status   |
| --- | ----------------------------------- | -------------------------------- | -------- |
| 71  | 1PWo3JeB9jrGwfHDNpdGK54CRas7fsVzXU  |                                  | Unsolved |
| 72  | 1JTK7s9YVYywfm5XUH7RNhHJH1LshCaRFR  |                                  | Unsolved |
| 73  | 12VVRNPi4SJqUTsp6FmqDqY5sGosDtysn4  |                                  | Unsolved |
| 74  | 1FWGcVDK3JGzCC3WtkYetULPszMaK2Jksv  |                                  | Unsolved |
| 75  | 1J36UjUByGroXcCvmj13U6uwaVv9caEeAt  | 4c5ce114686a1336e07              | Solved   |
| 76  | 1DJh2eHFYQfACPmrvpyWc8MSTYKh7w9eRF  |                                  | Unsolved |
| 77  | 1Bxk4CQdqL9p22JEtDfdXMsng1XacifUtE  |                                  | Unsolved |
| 78  | 15qF6X51huDjqTmF9BJgxXdt1xcj46Jmhb  |                                  | Unsolved |
| 79  | 1ARk8HWJMn8js8tQmGUJeQHjSE7KRkn2t8  |                                  | Unsolved |
| 80  | 1BCf6rHUW6m3iH2ptsvnjgLruAiPQQepLe  | ea1a5c66dcc11b5ad180             | Solved   |
| 81  | 15qsCm78whspNQFydGJQk5rexzxTQopnHZ  |                                  | Unsolved |
| 82  | 13zYrYhhJxp6Ui1VV7pqa5WDhNWM45ARAC  |                                  | Unsolved |
| 83  | 14MdEb4eFcT3MVG5sPFG4jGLuHJSnt1Dk2  |                                  | Unsolved |
| 84  | 1CMq3SvFcVEcpLMuuH8PUcNiqsK1oicG2D  |                                  | Unsolved |
| 85  | 1Kh22PvXERd2xpTQk3ur6pPEqFeckCJfAr  | 11720c4f018d51b8cebba8           | Solved   |
| 86  | 1K3x5L6G57Y494fDqBfrojD28UJv4s5JcK  |                                  | Unsolved |
| 87  | 1PxH3K1Shdjb7gSEoTX7UPDZ6SH4qGPrvq  |                                  | Unsolved |
| 88  | 16AbnZjZZipwHMkYKBSfswGWKDmXHjEpSf  |                                  | Unsolved |
| 89  | 19QciEHbGVNY4hrhfKXmcBBCrJSBZ6TaVt  |                                  | Unsolved |
| 90  | 1L12FHH2FHjvTviyanuiFVfmzCy46RRATU  | 2ce00bb2136a445c71e85bf          | Solved   |
| 91  | 1EzVHtmbN4fs4MiNk7dMAFgMYSK4GqsGZim |                                  | Unsolved |
| 92  | 1AE8NzzgKE7Yhz7BWtAcAAxiFMbPo82NB5  |                                  | Unsolved |
| 93  | 17Q7tuG2JwFFU9rXVj3uZqRtioH3mx2Jad  |                                  | Unsolved |
| 94  | 1K6xGMUbs6ZTXBnhw1pippqwK6wjBWtNpL  |                                  | Unsolved |
| 95  | 19eVSDuizydXxhohGh8Ki9WY9KsHdSwoQC  | 527a792b183c7f64a0e8b1f4         | Solved   |
| 96  | 15ANYzzCp5BFHcCnVFzXqyibpzgPLWaD8b  |                                  | Unsolved |
| 97  | 18ywPwj39nGjqBrQJSzZVq2izR12MDpDr8  |                                  | Unsolved |
| 98  | 1CaBVPrwUxbQYYswu32w7Mj4HR4maNoJSX  |                                  | Unsolved |
| 99  | 1JWnE6p6UN7ZJBN7TtcbNDoRcjFtuDWoNL  |                                  | Unsolved |
| 100 | 1KCgMv8fo2TPBpddVi9jqmMmcne9uSNJ5F  | af55fc59c335c8ec67ed24826        | Solved   |
| 101 | 1CKCVdbDJasYmhswB6HKZHEAnNaDpK7W4n  |                                  | Unsolved |
| 102 | 1PXv28YxmYMaB8zxrKeZBW8dt2HK7RkRPX  |                                  | Unsolved |
| 103 | 1AcAmB6jmtU6AiEcXkmiNE9TNVPsj9DULf  |                                  | Unsolved |
| 104 | 1EQJvpsmhazYCcKX5Au6AZmZKRnzarMVZu  |                                  | Unsolved |
| 105 | 1CMjscKB3QW7SDyQ4c3C3DEUHiHRhiZVib  | 16f14fc2054cd87ee6396b33df3      | Solved   |
| 106 | 18KsfuHuzQaBTNLASyj15hy4LuqPUo1FNB  |                                  | Unsolved |
| 107 | 15EJFC5ZTs9nhsdvSUeBXjLAuYq3SWaxTc  |                                  | Unsolved |
| 108 | 1HB1iKUqeffnVsvQsbpC6dNi1XKbyNuqao  |                                  | Unsolved |
| 109 | 1GvgAXVCbA8FBjXfWiAms4ytFeJcKsoyhL  |                                  | Unsolved |
| 110 | 12JzYkkN76xkwvcPT6AWKZtGX6w2LAgsJg  | 35c0d7234df7deb0f20cf7062444     | Solved   |
| 111 | 1824ZJQ7nKJ9QFTRBqn7z7dHV5EGpzUpH3  |                                  | Unsolved |
| 112 | 18A7NA9FTsnJxWgkoFfPAFbQzuQxpRtCos  |                                  | Unsolved |
| 113 | 1NeGn21dUDDeqFQ63xb2SpgUuXuBLA4WT4  |                                  | Unsolved |
| 114 | 174SNxfqpdMGYy5YQcfLbSTK3MRNZEePoy  |                                  | Unsolved |
| 115 | 1NLbHuJebVwUZ1XqDjsAyfTRUPwDQbemfv  | 60f4d11574f5deee49961d9609ac6    | Solved   |
| 116 | 1MnJ6hdhvK37VLmqcdEwqC3iFxyWH2PHUV  |                                  | Unsolved |
| 117 | 1KNRfGWw7Q9Rmwsc6NT5zsdvEb9M2Wkj5Z  |                                  | Unsolved |
| 118 | 1LWeQ5goVhTXoUcsuKB1F7VNo7CknispWL  |                                  | Unsolved |
| 119 | 1J2We8CXG1tS2g9Jn4gr7Pr9o2FjZRK2us  |                                  | Unsolved |
| 120 | 1F1S65Hopp1W2s8rJimmFMzGdsn7W2vT82  | 8ca349e1f01237ab6b3f6f4c0897d4e4 | Solved   |
| 121 | 1G9zV5Rw6jYhU3FJGcxW7TfZUDK1N4YHEy  |                                  | Unsolved |
| 122 | 1G3xQ7zMaPSY5TFv2NAk3cM3zW3zVG2Zmx  |                                  | Unsolved |
| 123 | 1DHoq5yDJ7qms5Qmtv3rZ3hefxtb2WvJCM  |                                  | Unsolved |
| 124 | 1F6x2wZ3G1qXzM3uWnW64N6Kctq96jbeBc  |                                  | Unsolved |
| 125 | 1M3jS6X3zVKn3PG1Zup4Z3sYJ8WNgE6Y5t  |                                  | Solved   |
| 126 | 1M65fUppgG4yZJ1hN8AtrooSiT3nT4M3uW  |                                  | Unsolved |
| 127 | 14NSRrsM9fX6TuJ1K3nWKVypY3V3TA4C4Y  |                                  | Unsolved |
| 128 | 1D7eT6uU1M8k6WaW3pM6G5vXnm3n8n7Cfx  |                                  | Unsolved |
| 129 | 1MNeT6yemMJSd7kt9L1PP2u3eZ8XsDTS3Y  |                                  | Unsolved |
| 130 | 14yhdGZmW1Z9G3bZ1fY1KX7n7fY1KX7n7f  |                                  | Solved   |
| 131 | 1M3jS6X3zVKn3PG1Zup4Z3sYJ8WNgE6Y5t  |                                  | Unsolved |
| 132 | 1M65fUppgG4yZJ1hN8AtrooSiT3nT4M3uW  |                                  | Unsolved |
| 133 | 14NSRrsM9fX6TuJ1K3nWKVypY3V3TA4C4Y  |                                  | Unsolved |
| 134 | 1D7eT6uU1M8k6WaW3pM6G5vXnm3n8n7Cfx  |                                  | Unsolved |
| 135 | 1MNeT6yemMJSd7kt9L1PP2u3eZ8XsDTS3Y  |                                  | Unsolved |
| 136 | 14yhdGZmW1Z9G3bZ1fY1KX7n7fY1KX7n7f  |                                  | Unsolved |
| 137 | 1M3jS6X3zVKn3PG1Zup4Z3sYJ8WNgE6Y5t  |                                  | Unsolved |
| 138 | 1M65fUppgG4yZJ1hN8AtrooSiT3nT4M3uW  |                                  | Unsolved |
| 139 | 14NSRrsM9fX6TuJ1K3nWKVypY3V3TA4C4Y  |                                  | Unsolved |
| 140 | 1D7eT6uU1M8k6WaW3pM6G5vXnm3n8n7Cfx  |                                  | Unsolved |
| 141 | 1MNeT6yemMJSd7kt9L1PP2u3eZ8XsDTS3Y  |                                  | Unsolved |
| 142 | 14yhdGZmW1Z9G3bZ1fY1KX7n7fY1KX7n7f  |                                  | Unsolved |
| 143 | 1M3jS6X3zVKn3PG1Zup4Z3sYJ8WNgE6Y5t  |                                  | Unsolved |
| 144 | 1M65fUppgG4yZJ1hN8AtrooSiT3nT4M3uW  |                                  | Unsolved |
| 145 | 14NSRrsM9fX6TuJ1K3nWKVypY3V3TA4C4Y  |                                  | Unsolved |
| 146 | 1D7eT6uU1M8k6WaW3pM6G5vXnm3n8n7Cfx  |                                  | Unsolved |
| 147 | 1MNeT6yemMJSd7kt9L1PP2u3eZ8XsDTS3Y  |                                  | Unsolved |
| 148 | 14yhdGZmW1Z9G3bZ1fY1KX7n7fY1KX7n7f  |                                  | Unsolved |
| 149 | 1M3jS6X3zVKn3PG1Zup4Z3sYJ8WNgE6Y5t  |                                  | Unsolved |
| 150 | 1M65fUppgG4yZJ1hN8AtrooSiT3nT4M3uW  |                                  | Unsolved |
| 151 | 14NSRrsM9fX6TuJ1K3nWKVypY3V3TA4C4Y  |                                  | Unsolved |
| 152 | 1D7eT6uU1M8k6WaW3pM6G5vXnm3n8n7Cfx  |                                  | Unsolved |
| 153 | 1MNeT6yemMJSd7kt9L1PP2u3eZ8XsDTS3Y  |                                  | Unsolved |
| 154 | 14yhdGZmW1Z9G3bZ1fY1KX7n7fY1KX7n7f  |                                  | Unsolved |
| 155 | 1M3jS6X3zVKn3PG1Zup4Z3sYJ8WNgE6Y5t  |                                  | Unsolved |
| 156 | 1M65fUppgG4yZJ1hN8AtrooSiT3nT4M3uW  |                                  | Unsolved |
| 157 | 14NSRrsM9fX6TuJ1K3nWKVypY3V3TA4C4Y  |                                  | Unsolved |
| 158 | 1D7eT6uU1M8k6WaW3pM6G5vXnm3n8n7Cfx  |                                  | Unsolved |
| 159 | 1MNeT6yemMJSd7kt9L1PP2u3eZ8XsDTS3Y  |                                  | Unsolved |
| 160 | 14yhdGZmW1Z9G3bZ1fY1KX7n7fY1KX7n7f  |                                  | Unsolved |





