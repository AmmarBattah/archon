import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def run(stdin):
    return subprocess.run(
        [sys.executable, str(HERE / "reports.py")],
        input=stdin, capture_output=True, text=True, check=False,
    )


class ReportsTest(unittest.TestCase):
    def test_totals_per_account_sorted(self):
        csv_text = (
            "date,account,amount_cents\n"
            "2026-03-01,rent,-120000\n"
            "2026-03-02,groceries,-4510\n"
            "2026-03-05,groceries,-990\n"
        )
        out = run(csv_text)
        self.assertEqual(out.returncode, 0, out.stderr)
        lines = out.stdout.splitlines()
        self.assertEqual(lines[1:], [line for line in sorted(lines[1:])])
        self.assertIn("groceries", lines[1])
        self.assertIn("-5500", lines[1])
        self.assertIn("rent", lines[2])


if __name__ == "__main__":
    unittest.main()
