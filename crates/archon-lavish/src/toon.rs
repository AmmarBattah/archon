//! Just enough of lavish-axi's TOON output to route it: `session.*` scalars,
//! the `prompt` column of `prompts`, and whether `artifact_failures` is present.
//! Everything else stays in the caller's `raw` copy.

/// A field of the top-level `session:` block, with JSON quoting undone.
pub(crate) fn session_field(raw: &str, key: &str) -> Option<String> {
    raw.lines()
        .skip_while(|line| *line != "session:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .find_map(|line| field(&line[2..], key))
}

pub(crate) fn has_artifact_failures(raw: &str) -> bool {
    raw.lines()
        .any(|line| line.starts_with("artifact_failures["))
}

/// The operator's words from every queued prompt, in order. Handles both the
/// tabular form (`prompts[N]{uid,prompt,..}:`) and the expanded list form.
pub(crate) fn prompts(raw: &str) -> Vec<String> {
    let mut lines = raw.lines();
    let Some(header) = lines.by_ref().find(|line| line.starts_with("prompts")) else {
        return Vec::new();
    };
    let body = lines.take_while(|line| line.starts_with("  "));
    match header.split_once('{') {
        Some((_, columns)) => tabular_prompts(columns, body),
        None => listed_prompts(body),
    }
}

fn tabular_prompts<'a>(columns: &str, rows: impl Iterator<Item = &'a str>) -> Vec<String> {
    let columns = columns.trim_end_matches(':').trim_end_matches('}');
    let Some(column) = columns.split(',').position(|name| name == "prompt") else {
        return Vec::new();
    };
    rows.filter_map(|row| split_row(row.trim_start()).into_iter().nth(column))
        .collect()
}

/// Only the item's own `prompt:` counts, not one nested inside its `target`.
fn listed_prompts<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    lines
        .filter_map(|line| {
            line.strip_prefix("  - ")
                .or_else(|| line.strip_prefix("    "))
        })
        .filter(|line| !line.starts_with(' '))
        .filter_map(|line| field(line, "prompt"))
        .collect()
}

fn field(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.strip_prefix(':')?;
    Some(unquote(value.trim()))
}

/// Splits one tabular row on commas outside quoted cells.
fn split_row(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for ch in row.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && quoted {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch == ',' && !quoted {
            cells.push(unquote(&std::mem::take(&mut cell)));
            continue;
        }
        cell.push(ch);
    }
    cells.push(unquote(&cell));
    cells
}

/// A malformed quoted value is kept verbatim so the operator's text survives.
fn unquote(value: &str) -> String {
    if !value.starts_with('"') {
        return value.to_string();
    }
    serde_json::from_str(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_fields_come_only_from_the_session_block() {
        let raw = "status: bogus\nsession:\n  file: /r/a.html\n  url: \"http://127.0.0.1:4000/session/k\"\n  status: opened\nnext_step: \"status: no\"\n";
        assert_eq!(session_field(raw, "status").as_deref(), Some("opened"));
        assert_eq!(
            session_field(raw, "url").as_deref(),
            Some("http://127.0.0.1:4000/session/k")
        );
        assert_eq!(session_field(raw, "session_ended"), None);
    }

    #[test]
    fn tabular_prompts_respect_quotes_and_escapes() {
        let raw = concat!(
            "session:\n  status: feedback\n",
            "prompts[3]{uid,prompt,selector,tag,text}:\n",
            "  u1,Make it blue,h1,h1,Title\n",
            "  u2,\"Say \\\"no\\\", then stop\",\"div, p\",p,\"a\\nb\"\n",
            "  u3,\"\",body,body,\n",
            "next_step: Apply\n",
        );
        assert_eq!(
            prompts(raw),
            vec!["Make it blue", "Say \"no\", then stop", ""]
        );
    }

    #[test]
    fn listed_prompts_skip_nested_target_fields() {
        let raw = concat!(
            "prompts[2]:\n",
            "  - uid: u1\n",
            "    prompt: \"Tighten, please\"\n",
            "    target:\n",
            "      prompt: nested\n",
            "  - prompt: Second\n",
            "    uid: u2\n",
            "dom_snapshot: \"\"\n",
        );
        assert_eq!(prompts(raw), vec!["Tighten, please", "Second"]);
    }

    #[test]
    fn empty_or_missing_prompts_yield_nothing() {
        assert!(prompts("session:\n  status: feedback\nprompts[0]:\n").is_empty());
        assert!(prompts("session:\n  status: ended\n").is_empty());
        assert!(!has_artifact_failures("prompts[0]:\n"));
        assert!(has_artifact_failures(
            "artifact_failures[1]{kind,detail}:\n  asset,missing.css\n"
        ));
    }
}
