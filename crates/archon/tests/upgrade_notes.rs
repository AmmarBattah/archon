//! A release whose store migration an older `archond` refuses says so in its
//! upgrade notes, and backs the store up before the step that updates.

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// The ways an upgrade note tells the operator to replace the binary.
const UPDATE_STEPS: [&str; 3] = [
    "`archon update`",
    "update the binary",
    "updating the binary",
];

/// Each release's `### Upgrading` text, whitespace collapsed, after its `## `
/// heading. Headings inside code blocks are text.
fn upgrade_notes(changelog: &str) -> Vec<(&str, String)> {
    let mut notes: Vec<(&str, Vec<&str>)> = Vec::new();
    let (mut release, mut upgrading, mut fenced) = ("", false, false);
    for line in changelog.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && line.starts_with("## ") {
            (release, upgrading) = (line, false);
        } else if !fenced && line.starts_with("### ") {
            upgrading = line == "### Upgrading";
            if upgrading {
                notes.push((release, Vec::new()));
            }
        } else if upgrading {
            let (_, words) = notes.last_mut().expect("pushed at its heading");
            words.extend(line.split_whitespace());
        }
    }
    assert!(!fenced, "a code block is never closed");
    notes
        .into_iter()
        .map(|(release, words)| (release, words.join(" ")))
        .collect()
}

#[test]
fn every_store_migration_is_one_way_and_backed_up_before_the_update() {
    let changelog = std::fs::read_to_string(format!("{ROOT}/CHANGELOG.md")).unwrap();
    let notes = upgrade_notes(&changelog);
    let migrating: Vec<_> = notes
        .iter()
        .filter(|(_, text)| text.contains("migrat"))
        .collect();
    assert!(
        migrating.iter().any(|(release, _)| *release == "## 1.1.0"),
        "the notes of 1.1.0, which migrated the store, are read: {notes:?}"
    );
    for (release, text) in migrating {
        assert!(text.contains("one-way"), "{release}: {text}");
        let backup = text
            .find("`archon.db`")
            .unwrap_or_else(|| panic!("{release} never backs up `archon.db`: {text}"));
        let update = UPDATE_STEPS
            .iter()
            .filter_map(|step| text.find(step))
            .min()
            .unwrap_or_else(|| panic!("{release} has no update step: {text}"));
        assert!(backup < update, "{release} backs up after updating: {text}");
    }
}

#[test]
fn upgrade_notes_are_read_per_release_outside_code_blocks() {
    let changelog = "## 2.0.0\n\n### Upgrading\n\nBack up\n  `archon.db`.\n\n\
                     ```md\n## not a release\n### Added\n```\n\nThen update.\n\n\
                     ### Fixed\n\n- No upgrade text.\n\n## 1.0.0\n\n### Upgrading\n\nNothing.\n";
    let notes = upgrade_notes(changelog);
    assert_eq!(
        notes,
        vec![
            (
                "## 2.0.0",
                "Back up `archon.db`. ```md ## not a release ### Added ``` Then update.".into()
            ),
            ("## 1.0.0", "Nothing.".into()),
        ]
    );
}
