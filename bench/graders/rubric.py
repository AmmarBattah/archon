"""Deterministic keyword rubric for answer tasks (ask / review).

Usage: python3 rubric.py RUBRIC_JSON

The text judged is the operator-visible answer ($BENCH_ANSWER) plus every
.md/.txt file each result repository ($BENCH_RESULT_*) added or changed
since the `bench-base` tag.

A rubric is {"window": W, "max_chars": M, "groups": [...]}. Each group has
`all`, a list of alternatives lists: it matches when one place in the text
has a match of every list within W characters of a match of the first list
(case-insensitive regexes). Requiring related facts to sit together means a
long answer that merely mentions every keyword somewhere does not score;
text longer than M characters fails outright for the same reason.

Prints {"score": matched/total} and exits 0 only when every group matched.
"""

import json
import os
import re
import subprocess
import sys

TEXT_SUFFIXES = (".md", ".txt")


def changed_text_files(repo):
    def git(*args):
        out = subprocess.run(
            ["git", *args], cwd=repo, check=True, capture_output=True, text=True
        )
        return out.stdout.splitlines()

    names = set(git("diff", "--name-only", "bench-base"))
    names.update(git("ls-files", "--others", "--exclude-standard"))
    return [os.path.join(repo, n) for n in sorted(names) if n.endswith(TEXT_SUFFIXES)]


def judged_text():
    parts = []
    answer = os.environ.get("BENCH_ANSWER")
    if answer and os.path.isfile(answer):
        with open(answer, encoding="utf-8", errors="replace") as f:
            parts.append(f.read())
    repos = [v for k, v in sorted(os.environ.items()) if k.startswith("BENCH_RESULT_")]
    assert repos, "grade.sh runs with at least one BENCH_RESULT_* repository"
    for repo in repos:
        for path in changed_text_files(repo):
            if os.path.isfile(path):
                with open(path, encoding="utf-8", errors="replace") as f:
                    parts.append(f.read())
    return "\n".join(parts)


def starts(patterns, text):
    """Start offsets of every match of any of `patterns`."""
    return [m.start() for p in patterns for m in re.finditer(p, text, re.IGNORECASE)]


def group_matches(alternatives, text, window):
    anchors = starts(alternatives[0], text)
    others = [starts(patterns, text) for patterns in alternatives[1:]]
    return any(
        all(any(abs(pos - anchor) <= window for pos in found) for found in others)
        for anchor in anchors
    )


def load_rubric(path):
    with open(path, encoding="utf-8") as f:
        rubric = json.load(f)
    groups, window, max_chars = rubric["groups"], rubric["window"], rubric["max_chars"]
    assert groups, "a rubric needs groups"
    assert all(g["all"] and all(g["all"]) for g in groups), "every group needs patterns"
    assert window > 0 and max_chars > 0, "window and max_chars must be positive"
    return groups, window, max_chars


def main():
    assert len(sys.argv) == 2, "usage: rubric.py RUBRIC_JSON"
    groups, window, max_chars = load_rubric(sys.argv[1])
    text = judged_text()
    if len(text) > max_chars:
        print(f"TOO LONG: {len(text)} characters judged, the limit is {max_chars}", file=sys.stderr)
        print(json.dumps({"score": 0}))
        sys.exit(1)
    matched = []
    for group in groups:
        hit = group_matches(group["all"], text, window)
        print(f"{'ok  ' if hit else 'MISS'} {group['name']}", file=sys.stderr)
        matched.append(hit)
    print(json.dumps({"score": sum(matched) / len(matched)}))
    sys.exit(0 if all(matched) else 1)


if __name__ == "__main__":
    main()
