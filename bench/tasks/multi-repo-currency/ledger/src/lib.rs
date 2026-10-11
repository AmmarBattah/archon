//! Transaction parsing and CSV rendering for the `ledger` exporter.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub date: String,
    pub account: String,
    pub amount_cents: i64,
}

pub const CSV_HEADER: &str = "date,account,amount_cents";

/// Parse one TSV line: `date<TAB>account<TAB>amount`.
pub fn parse_line(line: &str) -> Result<Entry, String> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() != 3 {
        return Err(format!("expected 3 tab-separated fields, got {}", fields.len()));
    }
    let date = fields[0].trim();
    if !is_iso_date(date) {
        return Err(format!("bad date {date:?}"));
    }
    let account = fields[1].trim();
    if account.is_empty() || account.contains(',') {
        return Err(format!("bad account {account:?}"));
    }
    Ok(Entry {
        date: date.to_string(),
        account: account.to_string(),
        amount_cents: parse_cents(fields[2].trim())?,
    })
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// `"12.34"` → 1234, `"-5"` → -500, `"0.5"` → 50.
pub fn parse_cents(s: &str) -> Result<i64, String> {
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    let valid = !whole.is_empty()
        && whole.bytes().all(|c| c.is_ascii_digit())
        && frac.len() <= 2
        && frac.bytes().all(|c| c.is_ascii_digit());
    if !valid {
        return Err(format!("bad amount {s:?}"));
    }
    let whole: i64 = whole.parse().map_err(|_| format!("amount {s:?} is too large"))?;
    let frac: i64 = format!("{frac:0<2}").parse().expect("two ascii digits");
    let cents = whole * 100 + frac;
    Ok(if negative { -cents } else { cents })
}

pub fn to_csv_row(entry: &Entry) -> String {
    format!("{},{},{}", entry.date, entry.account, entry.amount_cents)
}
