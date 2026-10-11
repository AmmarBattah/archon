use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use ledger::{parse_line, to_csv_row, CSV_HEADER};

fn main() -> ExitCode {
    let stdin = io::stdin();
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "{CSV_HEADER}").expect("writing stdout");
    for (index, line) in stdin.lock().lines().enumerate() {
        let line = line.expect("reading stdin");
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(&line) {
            Ok(entry) => writeln!(out, "{}", to_csv_row(&entry)).expect("writing stdout"),
            Err(e) => {
                eprintln!("line {}: {e}", index + 1);
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}
