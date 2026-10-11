use std::io::Write;
use std::process::{Command, Output, Stdio};

fn ledger(input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ledger"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect()
}

#[test]
fn currency_defaults_to_usd_and_is_carried_through() {
    let out = ledger("2026-03-01\trent\t-1200\n2026-03-02\tfood\t-4.5\tEUR\n# note\n\n2026-03-03\tpay\t3000.00\tUSD\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        stdout(&out),
        [
            "date,account,amount_cents,currency",
            "2026-03-01,rent,-120000,USD",
            "2026-03-02,food,-450,EUR",
            "2026-03-03,pay,300000,USD",
        ]
    );
}

#[test]
fn malformed_currency_exits_2_naming_the_line() {
    for bad in ["eur", "EU", "EURO", "E1R", "", "€UR"] {
        let out = ledger(&format!("2026-03-01\trent\t-1200\n2026-03-02\tfood\t-4.5\t{bad}\n"));
        assert_eq!(out.status.code(), Some(2), "currency {bad:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("line 2"), "currency {bad:?}: {err}");
    }
}

#[test]
fn other_errors_still_exit_1() {
    let out = ledger("2026-03-01\trent\tlots\tUSD\n");
    assert_eq!(out.status.code(), Some(1));
    let out = ledger("2026-03-01\trent\t1\tUSD\textra\n");
    assert_eq!(out.status.code(), Some(1));
}
