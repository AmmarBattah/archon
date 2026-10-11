//! Every link in the README, the docs and the community files that points
//! into this repository leads to a file in it, and every `#anchor` to a
//! heading of that file. Links to other sites are not followed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// Addresses of this repository's own files; what follows is `<ref>/<path>`.
const OWN_FILES: [&str; 3] = [
    "https://github.com/mengsig/archon/blob/",
    "https://github.com/mengsig/archon/tree/",
    "https://raw.githubusercontent.com/mengsig/archon/",
];
const ROOT_DOCUMENTS: [&str; 6] = [
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "CODE_OF_CONDUCT.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
];

/// The checked documents, relative to the repository root.
fn documents() -> Vec<PathBuf> {
    let mut documents: Vec<PathBuf> = ROOT_DOCUMENTS.iter().map(PathBuf::from).collect();
    for entry in std::fs::read_dir(Path::new(ROOT).join("docs")).unwrap() {
        let name = entry.unwrap().file_name();
        if Path::new(&name).extension().is_some_and(|ext| ext == "md") {
            documents.push(Path::new("docs").join(name));
        }
    }
    documents.sort();
    assert!(documents.len() > ROOT_DOCUMENTS.len(), "docs/ holds guides");
    documents
}

/// The lines of `text` outside fenced code blocks, with their numbers.
fn prose(text: &str) -> Vec<(usize, &str)> {
    let mut fenced = false;
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let start = line.trim_start();
        if start.starts_with("```") || start.starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced {
            lines.push((index + 1, line));
        }
    }
    assert!(!fenced, "a code block is never closed");
    lines
}

/// Where the links of one line lead: `[text](target)`, `![alt](target)` and
/// `[label]: target`. Code spans hold no links.
fn targets(line: &str) -> Vec<String> {
    let outside_code: String = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
    let mut targets: Vec<String> = outside_code
        .match_indices("](")
        .map(|(at, opening)| &outside_code[at + opening.len()..])
        .map(|rest| rest.split([')', ' ', '\t']).next().unwrap_or_default())
        .map(|target| target.trim_matches(['<', '>']).to_owned())
        .collect();
    let definition = outside_code.trim_start().strip_prefix('[');
    if let Some((_, target)) = definition.and_then(|rest| rest.split_once("]: ")) {
        targets.push(
            target
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    targets
}

/// The anchor GitHub gives a heading: lower case, punctuation dropped,
/// spaces as hyphens.
fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The anchors of a Markdown document; a repeated heading gets `-1`, `-2`.
fn anchors(text: &str) -> HashSet<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut anchors = HashSet::new();
    for (_, line) in prose(text) {
        let title = line.trim_start_matches('#');
        let level = line.len() - title.len();
        if !(1..=6).contains(&level) || !title.starts_with(' ') {
            continue;
        }
        let slug = slug(title);
        let repeats = seen.entry(slug.clone()).or_default();
        anchors.insert(match *repeats {
            0 => slug,
            n => format!("{slug}-{n}"),
        });
        *repeats += 1;
    }
    anchors
}

/// The path under the root that `target`, found in `document`, names, and
/// its anchor. `None` for a link to another site.
fn local(document: &Path, target: &str) -> Option<(PathBuf, Option<String>)> {
    let (address, anchor) = match target.split_once('#') {
        Some((address, anchor)) => (address, Some(anchor.to_owned())),
        None => (target, None),
    };
    let own = OWN_FILES.iter().find_map(|site| address.strip_prefix(site));
    if let Some(versioned) = own {
        let path = versioned.split_once('/').map(|(_ref, path)| path);
        return Some((PathBuf::from(path.unwrap_or_default()), anchor));
    }
    if address.contains("://") || address.starts_with("mailto:") {
        return None;
    }
    if address.is_empty() {
        return Some((document.to_path_buf(), anchor));
    }
    let directory = document.parent().expect("a document is a file");
    Some((directory.join(address), anchor))
}

/// What is wrong with `target` as a link in `document`, if anything.
fn broken(document: &Path, target: &str) -> Option<String> {
    assert!(document.is_relative(), "documents are named from the root");
    let (path, anchor) = local(document, target)?;
    let root = Path::new(ROOT).canonicalize().unwrap();
    let Ok(file) = root.join(&path).canonicalize() else {
        return Some(format!("{} does not exist", path.display()));
    };
    if !file.starts_with(&root) {
        return Some(format!("{} is outside the repository", path.display()));
    }
    let anchor = anchor?;
    if file.extension().is_none_or(|ext| ext != "md") {
        return None;
    }
    let text = std::fs::read_to_string(&file).unwrap();
    match anchors(&text).contains(&anchor) {
        true => None,
        false => Some(format!("{} has no heading #{anchor}", path.display())),
    }
}

#[test]
fn every_link_into_the_repository_leads_to_a_file_and_a_heading() {
    let (mut checked, mut problems) = (0, Vec::new());
    for document in documents() {
        let text = std::fs::read_to_string(Path::new(ROOT).join(&document)).unwrap();
        for (number, line) in prose(&text) {
            for target in targets(line) {
                checked += usize::from(local(&document, &target).is_some());
                if let Some(problem) = broken(&document, &target) {
                    problems.push(format!("{}:{number}: {problem}", document.display()));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "broken links:\n{}",
        problems.join("\n")
    );
    assert!(
        checked >= 80,
        "only {checked} links were found: is one missed?"
    );
}

#[test]
fn links_are_found_outside_code_and_headings_get_githubs_anchors() {
    let line = "See [a](one.md#x), ![b](<two.svg> \"title\") and `[c](code.md)`.";
    assert_eq!(targets(line), ["one.md#x", "two.svg"]);
    assert_eq!(targets("[label]: three.md \"title\""), ["three.md"]);
    assert_eq!(slug(" 13.2 Worktree GC"), "132-worktree-gc");
    assert_eq!(slug(" Who may dispatch?"), "who-may-dispatch");
    assert_eq!(slug(" `archon gc` & caches"), "archon-gc--caches");

    let text = "# One\n\n```sh\n# not a heading\n[x](absent.md)\n```\n## Two\n## Two\n#Three\n";
    let expected = ["one", "two", "two-1"].map(str::to_owned);
    assert_eq!(anchors(text), HashSet::from(expected));
    let lines: Vec<usize> = prose(text).iter().map(|(number, _)| *number).collect();
    assert_eq!(lines, [1, 2, 7, 8, 9]);
}

#[test]
fn a_missing_file_a_missing_heading_and_a_way_out_are_each_reported() {
    let guide = Path::new("docs/guide.md");
    assert_eq!(broken(guide, "configuration.md#disk-use"), None);
    assert_eq!(broken(guide, "../LICENSE"), None);
    assert_eq!(broken(guide, "https://example.com/absent.md#x"), None);
    let on_github = "https://github.com/mengsig/archon/blob/main/SECURITY.md#trust-model";
    assert_eq!(broken(Path::new("CHANGELOG.md"), on_github), None);

    let cases = [
        ("absent.md", "docs/absent.md does not exist"),
        (
            "#no-such-heading",
            "docs/guide.md has no heading #no-such-heading",
        ),
        ("configuration.md#Disk-use", "has no heading #Disk-use"),
        ("../../outside.md", "does not exist"),
        ("../..", "is outside the repository"),
        (
            "https://github.com/mengsig/archon/blob/v1.0.0/docs/absent.md",
            "docs/absent.md does not exist",
        ),
    ];
    for (target, problem) in cases {
        let found = broken(guide, target).unwrap_or_default();
        assert!(found.contains(problem), "{target}: {found:?}");
    }
}
