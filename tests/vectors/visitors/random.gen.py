"""Builds tests/vectors/visitors/random.gen.json, the batch of random.json: the templates of the parser's statement and
random batches (tests/vectors/parser/statements.gen.json, tags.gen.json, statements_random.gen.json and the first
expressions of expressions_random.gen.json), each with one environment configuration taken in turn from CONFIGS, for
visitors.gen.php.

Regenerate from the repository root:

    python -I tests/vectors/visitors/random.gen.py > tests/vectors/visitors/random.gen.json
    MSYS_NO_PATHCONV=1 docker run --rm -i -v "$PWD/tests/vectors/visitors:/v" -e CASES_FILE=/v/random.gen.json twig-oracle:3.31.1 php < tests/vectors/visitors/visitors.gen.php > tests/vectors/visitors/random.json
"""

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
PARSER = ROOT / "tests" / "vectors" / "parser"

CONFIGS = [
    {"config": {"autoescape": "html"}, "extensions": ["sandbox"]},
    {"config": {"autoescape": False}},
    {"config": {"autoescape": "js"}},
    {"config": {"autoescape": "name"}, "name": "index.html.twig"},
    {"config": {"autoescape": "html", "optimizations": 0}},
    {"config": {"autoescape": False}, "extensions": ["sandbox"]},
    {"config": {"autoescape": "css"}},
    {"config": {"autoescape": "html_attr", "optimizations": 2}, "extensions": ["sandbox"]},
]


def load(name, limit=None):
    cases = json.loads((PARSER / name).read_text(encoding="utf-8"))
    return cases if limit is None else cases[:limit]


def main():
    sources = []
    for name, limit in [
        ("statements.gen.json", None),
        ("tags.gen.json", None),
        ("statements_random.gen.json", None),
        ("expressions_random.gen.json", 300),
    ]:
        for case in load(name, limit):
            if "ext" in case:
                continue
            sources.append((name.split(".")[0] + ":" + case["id"], case))
    out = []
    for index, (case_id, case) in enumerate(sources):
        config = CONFIGS[index % len(CONFIGS)]
        extensions = sorted(set(case.get("extensions", [])) | set(config.get("extensions", [])))
        item = {"id": case_id, "source": case["source"], "config": config["config"]}
        if "name" in config:
            item["name"] = config["name"]
        elif "name" in case:
            item["name"] = case["name"]
        if extensions:
            item["extensions"] = extensions
        out.append(item)
    print(json.dumps(out, indent=1))


main()
