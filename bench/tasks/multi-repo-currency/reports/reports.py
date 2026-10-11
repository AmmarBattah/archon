"""Per-account totals from the ledger's CSV export.

Usage: python3 reports.py < ledger.csv
"""

import csv
import sys


def totals(rows):
    """{account: total_cents} from CSV dict rows."""
    result = {}
    for row in rows:
        account = row["account"]
        result[account] = result.get(account, 0) + int(row["amount_cents"])
    return result


def render(result):
    lines = ["account,total_cents"]
    for account in sorted(result):
        lines.append(f"{account},{result[account]}")
    return "\n".join(lines) + "\n"


def main():
    rows = csv.DictReader(sys.stdin)
    sys.stdout.write(render(totals(rows)))


if __name__ == "__main__":
    main()
