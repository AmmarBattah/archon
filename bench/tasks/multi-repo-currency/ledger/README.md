# ledger

Converts a transaction log to CSV for the reporting tools.

```sh
cargo run -q < transactions.tsv > ledger.csv
```

Input: one transaction per line, `date<TAB>account<TAB>amount` (ISO date,
amount with up to two decimals). Blank lines and `#` comments are skipped.

Output: `date,account,amount_cents`. Any malformed line stops the export
with `line N: ...` on stderr and exit status 1.
