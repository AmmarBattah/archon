use ledger::{parse_cents, parse_line, to_csv_row};

#[test]
fn parses_amounts() {
    assert_eq!(parse_cents("12.34"), Ok(1234));
    assert_eq!(parse_cents("-5"), Ok(-500));
    assert_eq!(parse_cents("0.5"), Ok(50));
    assert!(parse_cents("1.234").is_err());
    assert!(parse_cents("abc").is_err());
}

#[test]
fn parses_lines() {
    let entry = parse_line("2026-03-01\tgroceries\t-45.10").unwrap();
    assert_eq!(entry.account, "groceries");
    assert_eq!(entry.amount_cents, -4510);
    assert!(to_csv_row(&entry).starts_with("2026-03-01,groceries,-4510"));
    assert!(parse_line("2026-03-01\tgroceries").is_err());
    assert!(parse_line("March 1\tgroceries\t1").is_err());
}
