"""Prints the comparison table of research/benchmarks/README.md from the results of run-rust.sh and run-php.sh.

    python research/benchmarks/compare.py [results dir, default: research/benchmarks/results] [--before <dir>]

With `--before`, a copy of another run's `results` directory (run-rust.sh of an earlier commit), the table also
shows that run's twig-rs times and the speed-up.

Inputs: <results>/criterion/<group>/<scenario>/new/estimates.json (criterion's median, run-rust.sh) and
<results>/php.jsonl, <results>/php-jit.jsonl (bench.php's median, run-php.sh). Both are medians of per-iteration
sample means.
"""

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def php_results(path):
    results = {}
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                row = json.loads(line)
                results[(row["group"], row["name"])] = row["median_ns"]
    return results


def rust_results(root):
    results = {}
    for estimates in sorted(root.glob("*/*/new/estimates.json")):
        group, name = estimates.parts[-4], estimates.parts[-3]
        data = json.loads(estimates.read_text(encoding="utf-8"))
        results[(group, name)] = data["median"]["point_estimate"]
    return results


def fmt(ns):
    if ns is None:
        return "-"
    if ns >= 1e6:
        return f"{ns / 1e6:.2f} ms"
    return f"{ns / 1e3:.1f} µs"


def ratio(php, rust):
    if php is None or rust is None:
        return "-"
    if php >= rust:
        return f"{php / rust:.1f}x faster"
    return f"{rust / php:.1f}x slower"


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    args = sys.argv[1:]
    before = None
    if "--before" in args:
        i = args.index("--before")
        before = rust_results(pathlib.Path(args[i + 1]) / "criterion")
        del args[i : i + 2]
    results = pathlib.Path(args[0]) if args else HERE / "results"
    rust = rust_results(results / "criterion")
    php = php_results(results / "php.jsonl")
    jit = php_results(results / "php-jit.jsonl")
    keys = sorted(set(rust) | set(php) | set(jit))
    if before is None:
        print("| benchmark | twig-rs | PHP Twig | PHP Twig, OPcache + JIT | twig-rs vs PHP Twig | twig-rs vs OPcache + JIT |")
        print("|---|---:|---:|---:|---:|---:|")
        for key in keys:
            r, p, j = rust.get(key), php.get(key), jit.get(key)
            print(f"| {key[0]}/{key[1]} | {fmt(r)} | {fmt(p)} | {fmt(j)} | {ratio(p, r)} | {ratio(j, r)} |")
        return
    print(
        "| benchmark | twig-rs before | twig-rs | speed-up | PHP Twig | PHP Twig, OPcache + JIT"
        " | twig-rs vs PHP Twig | twig-rs vs OPcache + JIT |"
    )
    print("|---|---:|---:|---:|---:|---:|---:|---:|")
    for key in keys:
        b, r, p, j = before.get(key), rust.get(key), php.get(key), jit.get(key)
        speedup = f"{b / r:.2f}x" if b is not None and r is not None else "-"
        print(
            f"| {key[0]}/{key[1]} | {fmt(b)} | {fmt(r)} | {speedup} | {fmt(p)} | {fmt(j)}"
            f" | {ratio(p, r)} | {ratio(j, r)} |"
        )


if __name__ == "__main__":
    main()
