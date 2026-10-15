#!/usr/bin/env python3
"""Render published chart.json snapshots as one SVG and a numeric CSV."""

import argparse
import csv
import html
import json
import math
from pathlib import Path


ARMS = {"archon": "#225ea8", "aether": "#d97706"}
METRICS = [("pass_rate", "Pass rate (%)"), ("cost_usd", "Mean cost / trial ($)"),
           ("wall_secs", "Mean wall / trial (s)")]


def read_rows(directories):
    if not directories or len(set(directories)) != len(directories):
        raise ValueError("Provide distinct result directories")
    rows = []
    for directory in directories:
        data = json.loads((directory / "chart.json").read_text())
        selected = [arm for arm in data["arms"] if arm["arm"] in ARMS]
        if {arm["arm"] for arm in selected} != set(ARMS) or len(selected) != 2:
            raise ValueError(f"{directory} needs exactly one summary for each arm")
        for arm in selected:
            cost = arm["cost_usd"]
            if (arm["trials"] <= 0 or cost is None
                    or cost["n"] != arm["trials"] or cost["mean"] is None):
                raise ValueError(f"{directory} has empty or partly unpriced trials")
            # Timed-out trials graded on their committed work stay out of pass_rate.
            late_passes = arm.get("timeout_graded_passes", 0)
            if not 0 <= late_passes <= arm.get("timeout_graded", 0) <= arm["timeouts"]:
                raise ValueError(f"{directory} has inconsistent graded-on-timeout counts")
            row = dict(version=directory.name.removeprefix("results-"), arm=arm["arm"],
                       trials=arm["trials"], tasks=len(data["tasks"]),
                       pass_rate=100 * arm["success_rate"],
                       cost_usd=cost["mean"], wall_secs=arm["wall_secs"]["mean"],
                       graded_on_timeout_passes=late_passes,
                       cost_floor=bool(arm.get("cost_floor", False)))
            if any(not math.isfinite(row[key]) or row[key] < 0 for key, _ in METRICS):
                raise ValueError(f"{directory} has invalid metrics")
            rows.append(row)
    assert len(rows) == 2 * len(directories)
    assert all(0 <= row["pass_rate"] <= 100 for row in rows)
    return rows


def panel(rows, metric, title, offset):
    assert rows and len(rows) % 2 == 0
    assert metric in {name for name, _ in METRICS}
    maximum = 100 if metric == "pass_rate" else max(row[metric] for row in rows) * 1.15
    scale = 230 / maximum if maximum else 0
    parts = [f'<g transform="translate({offset},70)">',
             f'<text x="0" y="0" font-weight="600">{html.escape(title)}</text>']
    for index, row in enumerate(rows):
        y = 30 + index * 31
        value = row[metric]
        width = value * scale
        label = f'{row["version"]} {row["arm"]}'
        floor = "≥ " if metric == "cost_usd" and row["cost_floor"] else ""
        parts.extend([
            f'<text x="0" y="{y + 13}">{html.escape(label)}</text>',
            f'<rect x="108" y="{y}" width="{width:.2f}" height="19" fill="{ARMS[row["arm"]]}"/>',
            f'<text x="{114 + width:.2f}" y="{y + 13}">{floor}{value:.2f}</text>',
        ])
    return "\n".join(parts + ["</g>"])


def render(rows):
    assert len(rows) >= 2
    assert len({(row["version"], row["arm"]) for row in rows}) == len(rows)
    height = 160 + len(rows) * 31
    panels = [panel(rows, metric, title, 20 + index * 420)
              for index, (metric, title) in enumerate(METRICS)]
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="{height}" viewBox="0 0 1280 {height}" role="img">
<title>Archon benchmark history: pass rate, cost and wall time</title>
<desc>Historical snapshots use different models, task mixes and sample sizes. A complete smoke run establishes a separate quality baseline.</desc>
<style>text {{font: 12px system-ui,sans-serif; fill:#172033}}</style>
<rect width="1280" height="{height}" fill="#fff"/>
<text x="20" y="30" style="font-size:20px;font-weight:600">Benchmark history · Archon and single-agent Aether</text>
{''.join(panels)}
<text x="20" y="{height - 35}">Observed graded-trial means; infrastructure failures excluded. v1/v2 are incomplete and task weights differ. ≥ marks a cost Aether reported as incomplete.</text>
<text x="20" y="{height - 15}">Smoke uses Haiku and four tasks (two new); older snapshots use Opus. Compare like configurations. Small samples do not establish reliability.</text>
</svg>
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directories", nargs="+", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    rows = read_rows(args.directories)
    if args.output.suffix != ".svg":
        parser.error("--output must end in .svg")
    if not args.output.parent.is_dir():
        parser.error("--output parent directory must exist")
    args.output.write_text(render(rows))
    with args.output.with_suffix(".csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
