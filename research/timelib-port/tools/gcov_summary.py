#!/usr/bin/env python3
"""Summarises the *.gcov files written by harness/coverage.sh.

Usage: python -I gcov_summary.py <outdir> [--uncovered]

Prints executed/executable line counts per upstream source (parse_date.re and parse_iso_intervals.re are where
the scanner actions and helpers live; parse_date.c / parse_iso_intervals.c hold the generated DFA), and with
--uncovered every executable line that the corpus never reached, with its text.
"""
import os
import sys

SOURCES = ["parse_date.re", "parse_iso_intervals.re", "parse_tz.c", "parse_posix.c", "tm2unixtime.c",
           "unixtime2tm.c", "interval.c", "dow.c", "timelib.c", "parse_date.c", "parse_iso_intervals.c"]


def read_gcov(path):
    lines = []
    with open(path, encoding="latin-1") as fh:
        for raw in fh:
            parts = raw.rstrip("\n").split(":", 2)
            if len(parts) < 3:
                continue
            count, lineno, text = parts[0].strip(), parts[1].strip(), parts[2]
            if not lineno.isdigit() or lineno == "0":
                continue
            lines.append((count, int(lineno), text))
    return lines


def main():
    outdir = sys.argv[1]
    show = "--uncovered" in sys.argv
    files = {f[:-5]: os.path.join(outdir, f) for f in os.listdir(outdir) if f.endswith(".gcov")}
    for src in SOURCES:
        # gcov names the files after the path in #line directives (e.g. "ext#date#lib#parse_date.re.gcov").
        matches = [p for name, p in files.items() if name == src or name.endswith("#" + src)]
        if not matches:
            print("%-24s (no data)" % src)
            continue
        lines = read_gcov(matches[0])
        executable = [l for l in lines if l[0] != "-"]
        missed = [l for l in executable if l[0].startswith("#####") or l[0].startswith("=====")]
        pct = 100.0 * (len(executable) - len(missed)) / max(len(executable), 1)
        print("%-24s %6d / %6d lines executed (%.1f%%)" % (src, len(executable) - len(missed), len(executable), pct))
        if show and missed and src not in ("parse_date.c", "parse_iso_intervals.c"):
            for _, lineno, text in missed:
                print("    %5d: %s" % (lineno, text.strip()))


if __name__ == "__main__":
    main()
