#!/usr/bin/env python3
"""Wallet run-length generator that predicts future wallet candidates.

The generator recreates the methodology documented in analytics/wallet71_prediction_algorithm.md.
It blends regression and momentum on historical run-length shares, samples run budgets with a
Dirichlet prior, and walks an alternating run sequence using transition probabilities.
Use --target-index 70 to backtest against the known wallet 70 and use 71 (default) for prediction.
It can also pipe top candidates through the compiled bit-scan checker for on-the-fly validation.
"""

import argparse
import csv
import math
import os
import random
import subprocess
from collections import Counter, defaultdict
from typing import Dict, Iterable, List, Sequence, Tuple, Optional


def trim_leading_zeros(bit_string: str) -> str:
    trimmed = bit_string.lstrip("0")
    return trimmed or "0"


def compute_runs(bit_string: str) -> List[Tuple[str, int]]:
    if not bit_string:
        return []
    runs: List[Tuple[str, int]] = []
    current = bit_string[0]
    length = 1
    for char in bit_string[1:]:
        if char == current:
            length += 1
        else:
            runs.append((current, length))
            current = char
            length = 1
    runs.append((current, length))
    return runs


def run_key(bit_char: str, length: int) -> str:
    return f"{bit_char}x{length}"


def normalise_with_smoothing(counter: Counter, candidates: Sequence[str], smoothing: float) -> Dict[str, float]:
    weighted: Dict[str, float] = {}
    for key in candidates:
        weighted[key] = smoothing
    for key, value in counter.items():
        if key in weighted:
            weighted[key] += value
        else:
            weighted[key] = value + smoothing
    total = sum(weighted.get(key, 0.0) for key in candidates)
    if total <= 0.0:
        if candidates:
            uniform = 1.0 / len(candidates)
            return {key: uniform for key in candidates}
        return {}
    return {key: weighted.get(key, smoothing) / total for key in candidates}


def load_bit_sequences(csv_path: str) -> List[str]:
    bit_sequences: List[str] = []
    with open(csv_path, newline="") as handle:
        reader = csv.DictReader(handle)
        for row in reader:
            bits = trim_leading_zeros(row["bits"])
            bit_sequences.append(bits)
    return bit_sequences


def build_model(bit_sequences: Sequence[str]) -> Dict[str, object]:
    run_sequences = [compute_runs(bits) for bits in bit_sequences]
    ones_lengths = sorted({length for runs in run_sequences for bit, length in runs if bit == "1"})
    zeros_lengths = sorted({length for runs in run_sequences for bit, length in runs if bit == "0"})
    run_keys = [run_key("1", length) for length in ones_lengths] + [run_key("0", length) for length in zeros_lengths]
    run_lengths = {run_key("1", length): length for length in ones_lengths}
    run_lengths.update({run_key("0", length): length for length in zeros_lengths})

    share_rows: List[List[float]] = []
    start_counts: Counter = Counter()
    transition_counts: defaultdict = defaultdict(Counter)
    run_counts: Counter = Counter()
    run_bit_totals: Counter = Counter()
    fallback_counts = {"1": Counter(), "0": Counter()}
    total_bits = 0

    for runs in run_sequences:
        if not runs:
            share_rows.append([0.0 for _ in run_keys])
            continue
        total = sum(length for _, length in runs)
        total_bits += total
        share_dict = {key: 0.0 for key in run_keys}
        for bit, length in runs:
            key = run_key(bit, length)
            run_counts[key] += 1
            run_bit_totals[key] += length
            share_dict[key] = share_dict.get(key, 0.0) + (length / total)
        share_rows.append([share_dict.get(key, 0.0) for key in run_keys])
        first_key = run_key(runs[0][0], runs[0][1])
        start_counts[first_key] += 1
        for (bit, length), (next_bit, next_length) in zip(runs, runs[1:]):
            prev_key = run_key(bit, length)
            next_key = run_key(next_bit, next_length)
            transition_counts[prev_key][next_key] += 1
            fallback_counts[bit][next_key] += 1

    ones_keys = [run_key("1", length) for length in ones_lengths]
    zeros_keys = [run_key("0", length) for length in zeros_lengths]
    start_probs = normalise_with_smoothing(start_counts, ones_keys, smoothing=0.1)
    transition_probs: Dict[str, Dict[str, float]] = {}
    for key in run_keys:
        bit_type = key[0]
        candidates = zeros_keys if bit_type == "1" else ones_keys
        transition_probs[key] = normalise_with_smoothing(transition_counts.get(key, Counter()), candidates, smoothing=0.1)

    fallback_transitions = {
        "1": normalise_with_smoothing(fallback_counts["1"], zeros_keys, smoothing=0.1),
        "0": normalise_with_smoothing(fallback_counts["0"], ones_keys, smoothing=0.1),
    }

    global_bit_share: Dict[str, float] = {}
    if total_bits > 0:
        for key in run_keys:
            global_bit_share[key] = run_bit_totals.get(key, 0.0) / total_bits

    total_runs = sum(run_counts.values())
    run_frequency: Dict[str, float] = {}
    if total_runs > 0:
        for key in run_keys:
            run_frequency[key] = run_counts.get(key, 0) / total_runs

    start_fallback_counts = Counter({key: run_counts.get(key, 0) for key in ones_keys})
    start_fallback = normalise_with_smoothing(start_fallback_counts, ones_keys, smoothing=0.1)

    return {
        "run_sequences": run_sequences,
        "run_keys": run_keys,
        "ones_keys": ones_keys,
        "zeros_keys": zeros_keys,
        "run_lengths": run_lengths,
        "share_rows": share_rows,
        "start_probs": start_probs,
        "transition_probs": transition_probs,
        "fallback_transitions": fallback_transitions,
        "run_counts": run_counts,
        "run_frequency": run_frequency,
        "global_bit_share": global_bit_share,
        "total_bits": total_bits,
        "start_fallback": start_fallback,
    }


def regression_predict(xs: Sequence[int], ys: Sequence[float], target: int) -> float:
    n = len(xs)
    if n == 0:
        return 0.0
    if n == 1:
        return ys[0]
    mean_x = sum(xs) / n
    mean_y = sum(ys) / n
    numerator = sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys))
    denominator = sum((x - mean_x) ** 2 for x in xs)
    if denominator == 0:
        slope = 0.0
    else:
        slope = numerator / denominator
    intercept = mean_y - slope * mean_x
    return intercept + slope * target


def compute_target_shares(
    share_rows: Sequence[Sequence[float]],
    run_keys: Sequence[str],
    target_index: int,
    alpha: float,
    beta: float,
) -> Dict[str, float]:
    if not share_rows:
        uniform = 1.0 / len(run_keys)
        return {key: uniform for key in run_keys}
    xs = list(range(1, len(share_rows) + 1))
    reg_values: List[float] = []
    for col in range(len(run_keys)):
        ys = [row[col] for row in share_rows]
        reg_val = regression_predict(xs, ys, target_index)
        reg_values.append(max(reg_val, 0.0))
    last_row = share_rows[-1]
    if len(share_rows) >= 2:
        prev_row = share_rows[-2]
        delta = [curr - prev for curr, prev in zip(last_row, prev_row)]
    else:
        delta = [0.0 for _ in last_row]
    blended: List[float] = []
    for idx in range(len(run_keys)):
        momentum = last_row[idx] + beta * delta[idx]
        if momentum < 0.0:
            momentum = 0.0
        value = alpha * reg_values[idx] + (1.0 - alpha) * momentum
        if value < 0.0:
            value = 0.0
        blended.append(value)
    total = sum(blended)
    if total <= 0.0:
        fallback_total = sum(last_row)
        if fallback_total <= 0.0:
            uniform = 1.0 / len(run_keys)
            return {key: uniform for key in run_keys}
        return {key: last_row[idx] / fallback_total for idx, key in enumerate(run_keys)}
    return {key: blended[idx] / total for idx, key in enumerate(run_keys)}


def sample_dirichlet(alphas: Sequence[float], rng: random.Random) -> List[float]:
    samples: List[float] = []
    for alpha in alphas:
        if alpha <= 0.0:
            samples.append(0.0)
        else:
            samples.append(rng.gammavariate(alpha, 1.0))
    total = sum(samples)
    if total <= 0.0:
        if not alphas:
            return []
        uniform = 1.0 / len(alphas)
        return [uniform for _ in alphas]
    return [value / total for value in samples]


def weighted_choice(items: Sequence[str], weights: Sequence[float], rng: random.Random) -> str:
    total = sum(weights)
    if total <= 0.0:
        return rng.choice(list(items))
    threshold = rng.random() * total
    cumulative = 0.0
    for item, weight in zip(items, weights):
        cumulative += weight
        if cumulative >= threshold:
            return item
    return items[-1]


def compute_share_dict(sequence: Sequence[Tuple[str, int]]) -> Dict[str, float]:
    total = sum(length for _, length in sequence)
    if total == 0:
        return {}
    shares: Dict[str, float] = {}
    for bit, length in sequence:
        key = run_key(bit, length)
        shares[key] = shares.get(key, 0.0) + (length / total)
    return shares


def compute_count_penalty(actual_counts: Dict[str, float], expected_counts: Dict[str, float]) -> float:
    keys = set(actual_counts) | set(expected_counts)
    return sum(abs(actual_counts.get(key, 0.0) - expected_counts.get(key, 0.0)) for key in keys)


def compute_l1_distance(actual: Dict[str, float], target: Dict[str, float]) -> float:
    keys = set(actual) | set(target)
    return sum(abs(actual.get(key, 0.0) - target.get(key, 0.0)) for key in keys)


def hamming_distance(a: str, b: str) -> int:
    min_len = min(len(a), len(b))
    dist = sum(ch_a != ch_b for ch_a, ch_b in zip(a[:min_len], b[:min_len]))
    dist += abs(len(a) - len(b))
    return dist


class WalletPredictor:
    def __init__(self, model: Dict[str, object], target_shares: Dict[str, float], target_bits: int, params: Dict[str, float]):
        self.model = model
        self.target_shares = target_shares
        self.target_bits = target_bits
        self.run_keys = model["run_keys"]
        self.ones_keys = model["ones_keys"]
        self.zeros_keys = model["zeros_keys"]
        self.run_lengths = model["run_lengths"]
        self.start_probs = model["start_probs"]
        self.start_fallback = model["start_fallback"]
        self.transition_probs = model["transition_probs"]
        self.fallback_transitions = model["fallback_transitions"]
        self.global_bit_share = model["global_bit_share"]
        self.params = params
        self.base_expected_counts = {
            key: target_shares.get(key, 0.0) * target_bits / max(self.run_lengths.get(key, 1), 1)
            for key in self.run_keys
        }
        base_total = sum(self.base_expected_counts.values())
        scale = params.get("dirichlet_strength_scale", 1.0)
        self.dirichlet_strength = max(base_total / 4.0, 1.0) * scale
        self.dirichlet_alpha = self._build_dirichlet_alpha()

    def _build_dirichlet_alpha(self) -> List[float]:
        alphas: List[float] = []
        pseudo_strength = self.params.get("pseudocount_strength", 0.5)
        floor = self.params.get("alpha_floor", 1e-3)
        for key in self.run_keys:
            target_share = self.target_shares.get(key, 0.0)
            freq_share = self.global_bit_share.get(key, 0.0)
            alpha = target_share * self.dirichlet_strength + pseudo_strength * freq_share
            if alpha < floor:
                alpha = floor
            alphas.append(alpha)
        return alphas

    def get_start_prob(self, key: str) -> float:
        return self.start_probs.get(key, self.start_fallback.get(key, self.params["min_start"]))

    def get_transition_prob(self, prev_key: str, next_key: str) -> float:
        transitions = self.transition_probs.get(prev_key)
        if transitions and next_key in transitions:
            return transitions[next_key]
        bit_type = prev_key[0]
        fallback = self.fallback_transitions.get(bit_type, {})
        return fallback.get(next_key, self.params["min_transition"])

    def compute_balance_factor(self, candidate_length: int, remaining_bits: int, truncated_mode: bool) -> float:
        if remaining_bits <= 0:
            return self.params["min_weight"]
        leftover = remaining_bits - candidate_length
        if leftover >= 0:
            return 1.0
        penalty = self.params.get("truncation_penalty", 0.3)
        return math.exp(penalty * leftover)

    def negative_log_likelihood(self, sequence: Sequence[Tuple[str, int]]) -> float:
        if not sequence:
            return 0.0
        keys = [run_key(bit, length) for bit, length in sequence]
        first_prob = max(self.get_start_prob(keys[0]), self.params["min_transition"])
        neglog = -math.log(first_prob)
        for prev, curr in zip(keys, keys[1:]):
            prob = max(self.get_transition_prob(prev, curr), self.params["min_transition"])
            neglog += -math.log(prob)
        return neglog

    def generate_candidate(self, rng: random.Random) -> Dict[str, object]:
        share_sample = sample_dirichlet(self.dirichlet_alpha, rng)
        candidate_expected_counts = {
            key: share_sample[idx] * self.target_bits / max(self.run_lengths.get(key, 1), 1)
            for idx, key in enumerate(self.run_keys)
        }
        expected_remaining = dict(candidate_expected_counts)
        sequence: List[Tuple[str, int]] = []
        bits_used = 0
        prev_key: str = ""
        truncated_count = 0

        while bits_used < self.target_bits and len(sequence) < self.params["max_runs_cap"]:
            bit = "1" if len(sequence) % 2 == 0 else "0"
            candidate_keys_all = self.ones_keys if bit == "1" else self.zeros_keys
            remaining_bits = self.target_bits - bits_used
            filtered = [key for key in candidate_keys_all if self.run_lengths[key] <= remaining_bits]
            truncated_mode = False
            if filtered:
                candidate_keys = filtered
            else:
                candidate_keys = candidate_keys_all
                truncated_mode = True
            weights: List[float] = []
            for key in candidate_keys:
                length = self.run_lengths[key]
                exp_remaining = expected_remaining.get(key, 0.0)
                if exp_remaining > 0.0:
                    exp_weight = exp_remaining ** self.params["eta"]
                else:
                    exp_weight = (self.params["min_expected"]) ** self.params["eta"]
                    exp_weight *= math.exp(self.params["negative_expectation_penalty"] * exp_remaining)
                if not sequence:
                    base_prob = self.get_start_prob(key)
                else:
                    base_prob = self.get_transition_prob(prev_key, key)
                base_prob = max(base_prob, self.params["min_transition"])
                base_weight = base_prob ** self.params["gamma"]
                balance = self.compute_balance_factor(length, remaining_bits, truncated_mode)
                weight = max(base_weight * exp_weight * balance, self.params["min_weight"])
                weights.append(weight)
            if not weights:
                key_choice = rng.choice(candidate_keys_all)
            else:
                key_choice = weighted_choice(candidate_keys, weights, rng)
            length_choice = self.run_lengths[key_choice]
            actual_length = length_choice
            truncated = False
            if length_choice > remaining_bits:
                actual_length = remaining_bits
                truncated = True
            actual_key = run_key(bit, actual_length)
            expected_remaining.setdefault(actual_key, 0.0)
            expected_remaining[actual_key] -= 1.0
            sequence.append((bit, actual_length))
            bits_used += actual_length
            prev_key = actual_key
            if truncated:
                truncated_count += 1

        if bits_used != self.target_bits:
            return None
        return self.score_candidate(sequence, candidate_expected_counts, expected_remaining, share_sample, truncated_count)

    def score_candidate(
        self,
        sequence: Sequence[Tuple[str, int]],
        expected_counts: Dict[str, float],
        expected_remaining: Dict[str, float],
        share_sample: Sequence[float],
        truncated_count: int,
    ) -> Dict[str, object]:
        bitstring = "".join(bit * length for bit, length in sequence)
        run_counts = Counter(run_key(bit, length) for bit, length in sequence)
        run_shares = compute_share_dict(sequence)
        share_distance = compute_l1_distance(run_shares, self.target_shares)
        count_penalty = compute_count_penalty(run_counts, expected_counts)
        overuse_penalty = sum(max(-value, 0.0) for value in expected_remaining.values())
        neglog = self.negative_log_likelihood(sequence)
        total_runs = len(sequence)
        expected_total_runs = sum(expected_counts.values())
        run_total_penalty = abs(total_runs - expected_total_runs)
        score = (
            share_distance * self.params["share_weight"]
            + count_penalty * self.params["count_weight"]
            + overuse_penalty * self.params["overuse_weight"]
            + truncated_count * self.params["truncation_weight"]
            + neglog * self.params["loglik_weight"]
            + run_total_penalty * self.params["run_total_weight"]
        )
        return {
            "sequence": sequence,
            "bitstring": bitstring,
            "score": score,
            "run_counts": run_counts,
            "run_shares": run_shares,
            "share_distance": share_distance,
            "count_penalty": count_penalty,
            "overuse_penalty": overuse_penalty,
            "loglik": neglog,
            "truncated_runs": truncated_count,
            "expected_counts": expected_counts,
            "expected_total_runs": expected_total_runs,
            "run_total_penalty": run_total_penalty,
            "share_sample": list(share_sample),
        }


def generate_pool(predictor: WalletPredictor, pool_size: int, rng: random.Random) -> List[Dict[str, object]]:
    seen = set()
    candidates: List[Dict[str, object]] = []
    attempts = 0
    max_attempts = max(pool_size * 15, pool_size + 5)
    while len(candidates) < pool_size and attempts < max_attempts:
        candidate = predictor.generate_candidate(rng)
        attempts += 1
        if not candidate:
            continue
        fingerprint = candidate["bitstring"]
        if fingerprint in seen:
            continue
        seen.add(fingerprint)
        candidates.append(candidate)
    return candidates


def evaluate_candidate(candidate: Dict[str, object], actual_bitstring: str) -> Dict[str, float]:
    actual_runs = compute_runs(actual_bitstring)
    actual_shares = compute_share_dict(actual_runs)
    actual_counts = Counter(run_key(bit, length) for bit, length in actual_runs)
    metrics = {
        "hamming": hamming_distance(candidate["bitstring"], actual_bitstring),
        "share_l1": compute_l1_distance(candidate["run_shares"], actual_shares),
        "count_l1": compute_count_penalty(candidate["run_counts"], actual_counts),
    }
    try:
        metrics["decimal_gap"] = abs(int(candidate["bitstring"], 2) - int(actual_bitstring, 2))
    except ValueError:
        metrics["decimal_gap"] = float("nan")
    return metrics


def format_top_items(items: Iterable[Tuple[str, float]], limit: int) -> str:
    sorted_items = sorted(items, key=lambda kv: kv[1], reverse=True)
    return ", ".join(f"{key}:{value:.3f}" for key, value in sorted_items[:limit])


def format_run_counts(counter: Counter, limit: int) -> str:
    items = sorted(counter.items(), key=lambda kv: (-kv[1], kv[0]))
    return ", ".join(f"{key}:{value}" for key, value in items[:limit])


def run_check_command(binary: str, address: str, hex_value: str) -> Tuple[int, str, str]:
    try:
        completed = subprocess.run(
            [binary, 'check', address, hex_value],
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError as exc:
        raise SystemExit(f"Check binary '{binary}' not found: {exc}") from exc
    return completed.returncode, completed.stdout.strip(), completed.stderr.strip()


def main() -> None:
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    binary_name = 'bit-scan.exe' if os.name == 'nt' else 'bit-scan'
    default_check_binary = os.path.join(repo_root, 'target', 'release', binary_name)
    parser = argparse.ArgumentParser(description="Generate wallet bit-string candidates using run-length analytics.")
    parser.add_argument("--data", default="analytics/private_keys_1_70_bit.csv", help="CSV file with historical wallets (default: %(default)s)")
    parser.add_argument("--target-index", type=int, default=71, help="Wallet index to predict (default: 71)")
    parser.add_argument("--samples", type=int, default=200, help="Number of candidate sequences to sample (default: 200)")
    parser.add_argument("--top-k", type=int, default=5, help="How many best candidates to show (default: 5)")
    parser.add_argument("--seed", type=int, default=1729, help="Random seed for reproducibility")
    parser.add_argument("--alpha", type=float, default=0.6, help="Weight for regression projection in share blend")
    parser.add_argument("--beta", type=float, default=1.0, help="Momentum boost weight for latest share change")
    parser.add_argument("--gamma", type=float, default=1.0, help="Exponent applied to transition probabilities")
    parser.add_argument("--eta", type=float, default=0.8, help="Exponent applied to run budget weights")
    parser.add_argument("--dirichlet-strength-scale", type=float, default=1.0, help="Scale factor for Dirichlet concentration")
    parser.add_argument("--pseudocount-strength", type=float, default=0.5, help="Dirichlet pseudocount mass from historical bit share")
    parser.add_argument("--output-all", action="store_true", help="Print all sampled candidates instead of only the top-K")
    parser.add_argument("--check-address", help="Bitcoin address to validate candidates via bit-scan check")
    parser.add_argument("--check-binary", default=default_check_binary, help="Path to bit-scan executable (default: %(default)s)")
    parser.add_argument("--check-stop-on-success", action="store_true", help="Stop after the first candidate that passes --check-address")
    args = parser.parse_args()

    check_address: Optional[str] = args.check_address
    check_binary: Optional[str] = os.path.abspath(args.check_binary) if args.check_binary else None
    check_stop = args.check_stop_on_success

    if check_address:
        if not check_binary:
            raise SystemExit('--check-binary must be provided when --check-address is set')
        if not os.path.exists(check_binary):
            raise SystemExit(f"Check binary '{check_binary}' not found. Build the project (cargo build --release) or provide --check-binary.")

    csv_path = os.path.abspath(args.data)
    bit_sequences = load_bit_sequences(csv_path)
    if not bit_sequences:
        raise SystemExit("No bit sequences found in the provided CSV")

    total_wallets = len(bit_sequences)
    if args.target_index < 2:
        raise SystemExit("Target index must be at least 2")
    train_count = min(args.target_index - 1, total_wallets)
    training_sequences = bit_sequences[:train_count]
    model = build_model(training_sequences)

    if args.target_index <= total_wallets:
        target_bits = len(bit_sequences[args.target_index - 1])
    else:
        target_bits = args.target_index

    target_shares = compute_target_shares(model["share_rows"], model["run_keys"], args.target_index, args.alpha, args.beta)

    params = {
        "gamma": args.gamma,
        "eta": args.eta,
        "dirichlet_strength_scale": args.dirichlet_strength_scale,
        "pseudocount_strength": args.pseudocount_strength,
        "alpha_floor": 1e-3,
        "min_transition": 1e-4,
        "min_start": 1e-4,
        "min_weight": 1e-6,
        "min_expected": 0.05,
        "negative_expectation_penalty": 0.7,
        "truncation_penalty": 0.35,
        "share_weight": 1.0,
        "count_weight": 0.05,
        "overuse_weight": 0.1,
        "truncation_weight": 0.1,
        "loglik_weight": 0.01,
        "run_total_weight": 0.03,
        "max_runs_cap": 90,
    }

    predictor = WalletPredictor(model, target_shares, target_bits, params)
    rng = random.Random(args.seed)
    pool = max(args.samples, args.top_k)
    candidates = generate_pool(predictor, pool, rng)
    if not candidates:
        raise SystemExit("Failed to generate any candidates; consider adjusting parameters or increasing --samples")

    candidates.sort(key=lambda item: item["score"])
    to_display = candidates if args.output_all else candidates[: args.top_k]

    print(f"Loaded {total_wallets} wallets from {csv_path}")
    print(f"Training wallets: 1..{train_count}. Target index: {args.target_index} -> {target_bits} bits")
    print(f"Sampled {len(candidates)} candidates (requested {pool}). Displaying {len(to_display)}")
    print("Target share blend (top 6): " + format_top_items(target_shares.items(), limit=6))

    actual_bitstring = bit_sequences[args.target_index - 1] if args.target_index <= total_wallets else None

    for idx, candidate in enumerate(to_display, start=1):
        bitstring = candidate["bitstring"]
        hex_value_raw = format(int(bitstring, 2), "x")
        min_hex_len = (len(bitstring) + 3) // 4
        hex_value = hex_value_raw.zfill(min_hex_len)
        if len(hex_value) % 2 != 0:
            hex_value = "0" + hex_value
        print(f"[{idx}] score={candidate['score']:.4f} share_L1={candidate['share_distance']:.4f} runs={len(candidate['sequence'])} loglik={candidate['loglik']:.2f}")
        print(f"    bits={bitstring}")
        print(f"    hex={hex_value}")
        print(f"    run-counts: {format_run_counts(candidate['run_counts'], limit=6)}")
        if check_address:
            rc, out, err = run_check_command(check_binary, check_address, hex_value)
            message = out or err or ''
            message_line = message.splitlines()[0] if message else '(no output)'
            status = 'PASS' if rc == 0 else 'FAIL'
            print(f"    check[{status}] exit={rc}: {message_line}")
            if rc == 0 and check_stop:
                print('    stopping after successful check')
                return
        if actual_bitstring:
            metrics = evaluate_candidate(candidate, actual_bitstring)
            print(
                "    vs actual: "
                f"hamming={metrics['hamming']} share_L1={metrics['share_l1']:.4f} "
                f"count_L1={metrics['count_l1']:.2f} decimal_gap={metrics['decimal_gap']}"
            )


if __name__ == "__main__":
    main()
