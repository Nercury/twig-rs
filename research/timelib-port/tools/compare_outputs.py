#!/usr/bin/env python3
"""Compares the C harness and Rust dump outputs line by line, per harness command.

Usage: python -I compare_outputs.py <in.txt> <c.txt> <rust.txt> [examples-per-command]

Prints "<command> <lines> lines, <n> differ" for every command in the corpus and the first differing lines of
each command; exits with status 1 if anything differs or the line counts do not match.
"""
import sys
from collections import Counter


def main():
    inp = open(sys.argv[1], encoding="latin-1").read().split("\n")
    c = open(sys.argv[2], encoding="latin-1").read().split("\n")
    r = open(sys.argv[3], encoding="latin-1").read().split("\n")
    shown = int(sys.argv[4]) if len(sys.argv) > 4 else 3
    total, bad, examples = Counter(), Counter(), {}
    for i, line in enumerate(inp):
        if not line:
            continue
        mode = line.split(" ", 1)[0]
        total[mode] += 1
        a = c[i] if i < len(c) else None
        b = r[i] if i < len(r) else None
        if a != b:
            bad[mode] += 1
            examples.setdefault(mode, []).append(i)
    for mode in sorted(total):
        print("%-8s %8d lines, %6d differ" % (mode, total[mode], bad[mode]))
    print("%-8s %8d lines, %6d differ" % ("total", sum(total.values()), sum(bad.values())))
    for mode, idx in examples.items():
        for i in idx[:shown]:
            print("--- %s line %d" % (mode, i + 1))
            print("IN   " + inp[i][:300])
            print("C    " + (c[i] if i < len(c) else "<missing>")[:2000])
            print("RUST " + (r[i] if i < len(r) else "<missing>")[:2000])
    sys.exit(1 if bad or len(c) != len(r) else 0)


if __name__ == "__main__":
    main()
