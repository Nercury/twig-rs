"""Prints the comparison tables of research/benchmarks/README.md from the results of run-rust.sh and run-php.sh.

    python research/benchmarks/compare.py [<results dir>...] [--before <dir>]...

A results directory is research/benchmarks/results (the default) or a copy of it kept after a run. Given several (a
copy kept after each round of run-rust.sh and run-php.sh), the table shows the median of their times per benchmark.
With `--before`, a copy of the results of run-rust.sh on an earlier commit (repeated, one per round), the table also
shows those twig-rs times and the speed-up. Without PHP results (on Windows, where run-php.sh does not run) the PHP
columns are left out.

Inputs: <results>/criterion/<group>/<scenario>/new/estimates.json (criterion's median, run-rust.sh) and
<results>/php.jsonl, <results>/php-jit.jsonl (bench.php's median, run-php.sh). Both are medians of per-iteration
sample means.
"""

import json
import pathlib
import statistics
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


def median_of(runs):
    """Per benchmark, the median of the runs that measured it."""
    keys = set().union(*runs)
    return {key: statistics.median(run[key] for run in runs if key in run) for key in keys}


def fmt(ns):
    if ns is None:
        return "-"
    if ns >= 1e6:
        return f"{ns / 1e6:.2f} ms"
    if ns >= 1e4:
        return f"{ns / 1e3:.1f} µs"
    return f"{ns / 1e3:.2f} µs"


def ratio(php, rust):
    if php is None or rust is None:
        return "-"
    if php >= rust:
        return f"{php / rust:.2f}x faster"
    return f"{rust / php:.2f}x slower"


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    args = sys.argv[1:]
    before_dirs = []
    while "--before" in args:
        i = args.index("--before")
        before_dirs.append(pathlib.Path(args[i + 1]))
        del args[i : i + 2]
    dirs = [pathlib.Path(arg) for arg in args] or [HERE / "results"]
    rust = median_of([rust_results(d / "criterion") for d in dirs])
    php = median_of([php_results(d / "php.jsonl") for d in dirs])
    jit = median_of([php_results(d / "php-jit.jsonl") for d in dirs])
    before = median_of([rust_results(d / "criterion") for d in before_dirs]) if before_dirs else None
    with_php = bool(php or jit)

    head = ["benchmark"]
    head += ["twig-rs before", "twig-rs", "speed-up"] if before is not None else ["twig-rs"]
    if with_php:
        head += ["PHP Twig", "PHP Twig, OPcache + JIT", "twig-rs vs PHP Twig", "twig-rs vs OPcache + JIT"]
    print("| " + " | ".join(head) + " |")
    print("|---" + "|---:" * (len(head) - 1) + "|")
    for key in sorted(set(rust) | set(php) | set(jit) | set(before or {})):
        r = rust.get(key)
        cells = [f"{key[0]}/{key[1]}"]
        if before is not None:
            b = before.get(key)
            cells += [fmt(b), fmt(r), f"{b / r:.2f}x" if b is not None and r is not None else "-"]
        else:
            cells.append(fmt(r))
        if with_php:
            p, j = php.get(key), jit.get(key)
            cells += [fmt(p), fmt(j), ratio(p, r), ratio(j, r)]
        print("| " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
