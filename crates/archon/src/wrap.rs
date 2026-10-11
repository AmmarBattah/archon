//! Plain-text word wrapping for the chat and listings. Pure and
//! width-aware by character count.

/// Wrap `text` to `width` columns. Paragraph breaks and fenced code blocks
/// are kept verbatim, list items continue under their text, and words are
/// never split, so a long URL stays one clickable token.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    assert!(width > 0, "wrapping to zero columns loops forever");
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.trim_end().lines() {
        let line = line.trim_end();
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push(line.to_owned());
        } else if in_fence || line.is_empty() {
            out.push(line.to_owned());
        } else {
            wrap_line(line, width, &mut out);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    debug_assert!(out.iter().all(|l| !l.contains('\n')));
    out
}

/// Spacing between words is kept as written (a double space separates a
/// reference from the text); a wrap point drops it.
fn wrap_line(line: &str, width: usize, out: &mut Vec<String>) {
    let lead_len = line.len() - line.trim_start().len();
    let hang = hanging_indent(line);
    let mut current = line[..lead_len].to_owned();
    let mut current_len = current.chars().count();
    let mut at_line_start = true;
    let mut gap = 0;
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        rest = after.trim_start();
        let len = word.chars().count();
        if !at_line_start && current_len + gap + len > width {
            out.push(std::mem::replace(&mut current, " ".repeat(hang)));
            current_len = hang;
            at_line_start = true;
        }
        if !at_line_start {
            current.push_str(&" ".repeat(gap));
            current_len += gap;
        }
        current.push_str(word);
        current_len += len;
        at_line_start = false;
        gap = after[..after.len() - rest.len()].chars().count();
    }
    out.push(current);
}

/// Where continuation lines start: under the text of a list item
/// (`- `, `* `, `• `, `1. `, `2) `), else under the line's own indent.
fn hanging_indent(line: &str) -> usize {
    let trimmed = line.trim_start();
    let lead = line.chars().count() - trimmed.chars().count();
    let bullet = ["- ", "* ", "• "]
        .iter()
        .find(|b| trimmed.starts_with(*b))
        .map(|b| b.chars().count());
    let numbered = || {
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        let rest = &trimmed[digits..];
        (digits > 0 && (rest.starts_with(". ") || rest.starts_with(") "))).then_some(digits + 2)
    };
    lead + bullet.or_else(numbered).unwrap_or(0)
}

/// A trusted OSC-8 sequence. Reject controls and unsupported schemes rather
/// than letting a report location inject terminal commands or open a custom app.
pub fn report_link(location: &str) -> Option<String> {
    if location
        .chars()
        .any(|c| c.is_control() || is_bidi_control(c))
    {
        return None;
    }
    let target = if location.starts_with('/') {
        let mut uri = String::from("file://");
        for byte in location.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                use std::fmt::Write as _;
                let _ = write!(uri, "%{byte:02X}");
            }
        }
        uri
    } else if location.starts_with("https://") || location.starts_with("http://") {
        location.replace(' ', "%20")
    } else {
        return None;
    };
    Some(format!("\x1b]8;;{target}\x1b\\open report\x1b]8;;\x1b\\"))
}

/// Remove terminal controls from untrusted text (agent output can contain
/// escapes that would rewrite the operator's screen). C0/C1 controls, ESC
/// included, and bidi overrides go; tabs become spaces; newlines stay.
pub fn sanitize(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => clean.push('\n'),
            '\t' => clean.push_str("    "),
            c if c.is_control() || is_bidi_control(c) => {}
            c => clean.push(c),
        }
    }
    debug_assert!(!clean.contains('\x1b') && !clean.contains('\r'));
    debug_assert!(!clean.chars().any(is_bidi_control));
    clean
}

/// [`sanitize`] for text shown on one line, such as a table cell: a line
/// break becomes a space so it cannot fake the next row.
pub fn sanitize_line(text: &str) -> String {
    let line = sanitize(text).replace('\n', " ");
    debug_assert!(!line.contains('\n'));
    line
}

/// Characters that reorder how the rest of a line displays (Trojan Source).
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_word_boundaries_within_width() {
        let lines = wrap("the quick brown fox jumps over the lazy dog", 15);
        assert_eq!(lines, ["the quick brown", "fox jumps over", "the lazy dog"]);
        assert!(lines.iter().all(|l| l.chars().count() <= 15));
    }

    #[test]
    fn keeps_paragraphs_fences_and_long_words() {
        let text = "one two\n\n```\nlet x = a_very_long_line_of_code_here;\n```\nhttps://example.com/a/very/long/url";
        let lines = wrap(text, 10);
        assert_eq!(lines[0], "one two");
        assert_eq!(lines[1], "");
        assert_eq!(lines[3], "let x = a_very_long_line_of_code_here;");
        assert_eq!(lines[5], "https://example.com/a/very/long/url");
    }

    #[test]
    fn list_items_hang_under_their_text() {
        let lines = wrap("- alpha beta gamma\n12. delta epsilon", 12);
        assert_eq!(
            lines,
            ["- alpha beta", "  gamma", "12. delta", "    epsilon"]
        );
    }

    #[test]
    fn spacing_is_kept_within_a_line_and_dropped_at_a_wrap() {
        assert_eq!(wrap("done  [qi_1]", 40), ["done  [qi_1]"]);
        assert_eq!(wrap("done  [qi_1]", 8), ["done", "[qi_1]"]);
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        assert_eq!(wrap("", 20), [""]);
        assert_eq!(wrap("  \n", 20), [""]);
    }

    #[test]
    fn sanitize_strips_escapes_but_keeps_newlines() {
        assert_eq!(sanitize("a\x1b[2Jb\n\tc\r"), "a[2Jb\n    c");
    }

    #[test]
    fn sanitize_strips_c1_controls_osc_and_bidi_overrides() {
        let osc52 = "\x1b]52;c;cm0gLXJm\x07copied";
        assert_eq!(sanitize(osc52), "]52;c;cm0gLXJmcopied");
        assert_eq!(sanitize("a\u{9b}2Jb\u{85}c\x7f"), "a2Jbc");
        assert_eq!(
            sanitize("ok \u{202e}evil\u{2066}x\u{2069}\u{200f}"),
            "ok evilx"
        );
        assert_eq!(sanitize("naïve café ✓"), "naïve café ✓");
    }

    #[test]
    fn report_targets_encode_paths_and_refuse_controls_or_custom_schemes() {
        assert!(
            report_link("https://example.test/report")
                .unwrap()
                .contains("https://example.test/report")
        );
        assert!(
            report_link("/a b/#x?%.html")
                .unwrap()
                .contains("file:///a%20b/%23x%3F%25.html")
        );
        for path in [
            "",
            "relative.html",
            "javascript:alert(1)",
            "file:///x",
            "/x\nnew",
            "/x\u{202e}evil",
            "https://x/\x07",
        ] {
            assert_eq!(report_link(path), None, "{path:?}");
        }
    }

    #[test]
    fn sanitize_line_flattens_line_breaks() {
        assert_eq!(sanitize_line("title\nqi_x  queued"), "title qi_x  queued");
        assert_eq!(sanitize_line("a\r\nb\x1b[1G"), "a b[1G");
    }
}
