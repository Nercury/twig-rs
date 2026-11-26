"""Writes the context of every benchmark scenario: benches/scenarios/<name>/data.json.

The data is deterministic (no randomness), ASCII-keyed and written with sorted keys, so that the order of every
object is the same for PHP's json_decode() and for serde_json (whose maps are sorted by key).

    python research/benchmarks/gen_data.py
"""

import json
import pathlib

SCENARIOS = pathlib.Path(__file__).resolve().parents[2] / "benches" / "scenarios"

WORDS = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu"]

STRINGS = [
    "plain text",
    "<script>alert('x')</script>",
    "Tom & Jerry",
    "\"double\" and 'single' quotes",
    "naïve café résumé",
    "日本語のテキスト",
    "emoji \U0001f600 \U0001f389",
    "path/to?x=1&y=2#frag",
    "line\nbreak\ttab",
    "</style><!-- comment -->",
]


def loops():
    rows = []
    for i in range(1, 101):
        rows.append({
            "name": f"row {i}",
            "active": i % 3 == 0,
            "cells": [(i * 7 + j * 13) % 1000 for j in range(10)],
        })
    return {"rows": rows}


def inheritance():
    return {
        "site": {"name": "Example & Co", "tagline": "Templates <fast> & exact", "css": "/static/site.css?v=3"},
        "year": 2026,
        "menu": [
            {"href": f"/section/{i}", "label": f"Section {i}", "active": i == 3}
            for i in range(1, 9)
        ],
        "page": {
            "title": "Benchmark <page>",
            "items": [
                {
                    "id": i,
                    "title": f"Item {i}: {WORDS[i % len(WORDS)]}",
                    "body": f"Lorem ipsum dolor sit amet, <b>item {i}</b> & friends, \"quoted\" text.",
                    "tags": WORDS[i % 5:i % 5 + i % 4],
                }
                for i in range(1, 21)
            ],
        },
    }


def macros():
    return {
        "fields": [
            {"label": f"Field {i}", "name": f"field_{i}", "value": f"value \"{i}\" <{WORDS[i % len(WORDS)]}>"}
            for i in range(1, 101)
        ]
    }


def escaping():
    return {"strings": [f"{STRINGS[i % len(STRINGS)]} #{i}" for i in range(200)]}


def filters():
    products = []
    for i in range(1, 51):
        products.append({
            "id": i,
            "name": f"product {WORDS[i % len(WORDS)]} number {i}",
            "desc": f"<p>Description of <em>product {i}</em>.\nSecond line with {WORDS[(i * 5) % len(WORDS)]}.</p>",
            "price": round(i * 3.37 + 0.99, 2),
            "qty": 3 - i % 7,
            "sku": f"SKU-{i:04d}-{WORDS[i % len(WORDS)][:2].upper()}",
            "tags": [WORDS[(i + k) % len(WORDS)] for k in range(i % 5 + 1)],
            "attrs": {"color": ["red", "green", "blue"][i % 3], "size": ["S", "M", "L", "XL"][i % 4], "weight": i * 10},
        })
    return {"products": products}


def dates():
    return {"timestamps": [1_700_000_000 + i * 86_400 * 17 + i * 3_671 for i in range(100)]}


def main():
    for name, build in [("loops", loops), ("inheritance", inheritance), ("macros", macros), ("escaping", escaping),
                        ("filters", filters), ("dates", dates)]:
        path = SCENARIOS / name / "data.json"
        text = json.dumps(build(), sort_keys=True, ensure_ascii=True, indent=1) + "\n"
        path.write_bytes(text.encode("ascii"))
        print(f"{path}: {len(text)} bytes")


if __name__ == "__main__":
    main()
