"""Offline checks for published-snapshot history rendering."""

import csv
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import history


class HistoryTests(unittest.TestCase):
    def write_chart(self, root, priced=1):
        arms = [dict(arm=arm, trials=1, success_rate=1.0, timeouts=0,
                     cost_usd=dict(n=priced, mean=0.1), wall_secs=dict(mean=2.0))
                for arm in history.ARMS]
        (root / "chart.json").write_text(json.dumps(dict(arms=arms, tasks=[{}])))

    def test_render_preserves_metrics_and_comparison_caveats(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_chart(root)
            rows = history.read_rows([root])
            self.assertEqual([row["pass_rate"] for row in rows], [100, 100])
            self.assertEqual([row["cost_usd"] for row in rows], [0.1, 0.1])
            svg = history.render(rows)
            self.assertIn("different models, task mixes", svg)
            self.assertIn("Smoke uses Haiku", svg)

    def test_cost_bounds_are_marked_in_the_chart_and_csv_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_chart(root)
            data = json.loads((root / "chart.json").read_text())
            data["arms"][0]["cost_bound"] = "incomplete"
            data["arms"][1]["cost_bound"] = "floor"
            (root / "chart.json").write_text(json.dumps(data))
            rows = history.read_rows([root])
            self.assertEqual([row["cost_bound"] for row in rows], ["incomplete", "floor"])
            svg = history.render(rows)
            self.assertEqual(svg.count("≥ 0.10"), 1)
            self.assertEqual(svg.count("* 0.10"), 1)
            self.assertIn("≥ marks a cost Aether reported as incomplete", svg)
            self.assertIn("together with estimates; not a bound", svg)
            output = root / "history.svg"
            with mock.patch("sys.argv", ["history.py", str(root), "--output", str(output)]):
                history.main()
            with output.with_suffix(".csv").open() as stream:
                table = list(csv.DictReader(stream))
            self.assertEqual([row["cost_bound"] for row in table], ["incomplete", "floor"])

    def test_a_chart_without_cost_bound_loads_exact_and_a_bad_one_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_chart(root)
            rows = history.read_rows([root])
            self.assertEqual([row["cost_bound"] for row in rows], ["exact", "exact"])
            self.assertNotIn("≥ 0.10", history.render(rows))
            data = json.loads((root / "chart.json").read_text())
            data["arms"][0]["cost_bound"] = "maybe"
            (root / "chart.json").write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                history.read_rows([root])

    def test_graded_on_timeout_passes_stay_apart_from_the_pass_rate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_chart(root)
            data = json.loads((root / "chart.json").read_text())
            slow = data["arms"][0]
            slow.update(success_rate=0.0, timeouts=1, timeout_graded=1,
                        timeout_graded_passes=1)
            (root / "chart.json").write_text(json.dumps(data))
            rows = history.read_rows([root])
            self.assertEqual([row["pass_rate"] for row in rows], [0, 100])
            self.assertEqual([row["graded_on_timeout_passes"] for row in rows], [1, 0])
            self.assertNotIn("graded", history.render(rows).replace("graded-trial", ""))
            slow["timeout_graded_passes"] = 2
            (root / "chart.json").write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError, "graded-on-timeout"):
                history.read_rows([root])

    def test_incomplete_costs_and_duplicate_snapshots_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_chart(root, priced=0)
            with self.assertRaisesRegex(ValueError, "partly unpriced"):
                history.read_rows([root])
            with self.assertRaisesRegex(ValueError, "distinct"):
                history.read_rows([root, root])

    def test_null_cost_and_null_cost_mean_raise_value_error(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for cost in [None, dict(n=1, mean=None)]:
                self.write_chart(root)
                data = json.loads((root / "chart.json").read_text())
                data["arms"][0]["cost_usd"] = cost
                (root / "chart.json").write_text(json.dumps(data))
                with self.assertRaisesRegex(ValueError, "partly unpriced"):
                    history.read_rows([root])


if __name__ == "__main__":
    unittest.main()
