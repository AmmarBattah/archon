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


class CurrencyReportsTest(unittest.TestCase):
    def test_totals_per_account_and_currency(self):
        out = run(
            "date,account,amount_cents,currency\n"
            "2026-03-01,travel,-5000,EUR\n"
            "2026-03-02,travel,-2500,USD\n"
            "2026-03-03,food,-450,EUR\n"
            "2026-03-04,travel,-1000,EUR\n"
            "2026-03-05,food,-100,CHF\n"
        )
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(
            out.stdout.splitlines(),
            [
                "account,currency,total_cents",
                "food,CHF,-100",
                "food,EUR,-450",
                "travel,EUR,-6000",
                "travel,USD,-2500",
            ],
        )

    def test_old_csv_counts_as_usd(self):
        out = run("date,account,amount_cents\n2026-03-01,rent,-120000\n2026-03-09,rent,-5\n")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout.splitlines(), ["account,currency,total_cents", "rent,USD,-120005"])

    def test_malformed_currency_exits_2(self):
        for bad in ("eur", "EURO", "E1", ""):
            out = run(f"date,account,amount_cents,currency\n2026-03-01,rent,-1,{bad}\n")
            self.assertEqual(out.returncode, 2, bad)
            self.assertTrue(out.stderr.strip(), bad)


if __name__ == "__main__":
    unittest.main()
