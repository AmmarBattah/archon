//! Submodules populated in a team worktree. `status`, `diff` and `add`
//! recurse into them and run the filters their git config names, so the
//! daemon runs those only while every populated submodule's git directory
//! is one git keeps under the repository's common directory. A repository
//! a role made inside its worktree could name any command in its config.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use archon_core::error::ArchonError;

use super::git_output;
use crate::worktree::{git_failure, toplevel};

/// Deepest submodule nesting checked; anything deeper is refused.
const MAX_DEPTH: usize = 8;

/// The populated submodules of a team worktree, at any depth, named
/// relative to it.
pub(super) struct Submodules {
    /// Those git must not enter: see [`scan`].
    pub foreign: Vec<String>,
    /// Those whose git directory the repository keeps, with that directory.
    pub kept: Vec<(String, PathBuf)>,
}

/// The populated submodules in the worktree `tree`. Foreign are those git
/// must not enter: a symlink, a `.git` that is a directory or symlink, a
/// gitfile naming a git directory outside `<common>/modules/` and
/// `<common>/worktrees/<name>/modules/`, or one whose work tree git finds
/// elsewhere. Reads only indexes and gitfiles.
pub(super) fn scan(tree: &Path, common: &Path) -> Result<Submodules, ArchonError> {
    assert!(tree.is_absolute(), "team worktrees are absolute");
    assert!(common.is_absolute(), "common git directories are canonical");
    let mut found = Submodules {
        foreign: Vec::new(),
        kept: Vec::new(),
    };
    let mut pending = vec![(tree.to_owned(), String::new(), 1)];
    while let Some((dir, prefix, depth)) = pending.pop() {
        for raw in gitlinks(&dir)? {
            let Ok(link) = String::from_utf8(raw.clone()) else {
                let name = format!("{prefix}{}", String::from_utf8_lossy(&raw));
                found.foreign.push(name);
                continue;
            };
            let name = format!("{prefix}{link}");
            let path = dir.join(&link);
            match populated(&path, common)? {
                Populated::No => {}
                Populated::Kept(git_dir) if depth < MAX_DEPTH => {
                    pending.push((path, format!("{name}/"), depth + 1));
                    found.kept.push((name, git_dir));
                }
                Populated::Kept(_) | Populated::Foreign => found.foreign.push(name),
            }
        }
    }
    debug_assert!(found.foreign.iter().all(|name| !name.is_empty()));
    debug_assert!(found.kept.iter().all(|(_, dir)| kept_by(dir, common)));
    Ok(found)
}

/// What is at a gitlink's path, as git would treat it.
enum Populated {
    /// Nothing git would enter: no directory, or one without `.git`.
    No,
    /// A submodule whose git directory, canonical, the repository keeps.
    Kept(PathBuf),
    /// Anything else git could enter.
    Foreign,
}

/// Classify the gitlink path `path`.
fn populated(path: &Path, common: &Path) -> Result<Populated, ArchonError> {
    assert!(path.is_absolute(), "gitlink paths are joined to a worktree");
    let Some(found) = lstat(path)? else {
        return Ok(Populated::No);
    };
    if found.file_type().is_symlink() {
        return Ok(Populated::Foreign);
    }
    if !found.is_dir() {
        return Ok(Populated::No);
    }
    let gitfile = path.join(".git");
    let Some(dot_git) = lstat(&gitfile)? else {
        return Ok(Populated::No);
    };
    if !dot_git.file_type().is_file() {
        return Ok(Populated::Foreign);
    }
    let text = std::fs::read_to_string(&gitfile)
        .map_err(|e| ArchonError::io(format!("reading {}", gitfile.display()), e))?;
    let Some(named) = text.lines().next().and_then(|l| l.strip_prefix("gitdir: ")) else {
        return Ok(Populated::Foreign);
    };
    let Ok(git_dir) = std::fs::canonicalize(path.join(named)) else {
        return Ok(Populated::Foreign);
    };
    if !kept_by(&git_dir, common) {
        return Ok(Populated::Foreign);
    }
    // Its config may point git at another work tree (`core.worktree`).
    let own = std::fs::canonicalize(path)
        .map_err(|e| ArchonError::io(format!("resolving {}", path.display()), e))?;
    match toplevel(path)? == own {
        true => Ok(Populated::Kept(git_dir)),
        false => Ok(Populated::Foreign),
    }
}

/// Whether the canonical `git_dir` is a submodule's git directory that
/// the repository keeps: under `<common>/modules/`, or a worktree's
/// `<common>/worktrees/<name>/modules/`.
fn kept_by(git_dir: &Path, common: &Path) -> bool {
    let Ok(inside) = git_dir.strip_prefix(common) else {
        return false;
    };
    let parts: Vec<Component> = inside.components().collect();
    let named = |part: &Component, name: &str| matches!(part, Component::Normal(n) if *n == name);
    match parts.as_slice() {
        [first, _, ..] if named(first, "modules") => true,
        [first, Component::Normal(_), third, _, ..] => {
            named(first, "worktrees") && named(third, "modules")
        }
        _ => false,
    }
}

/// The gitlink paths in the index of the checkout `dir`, exactly as named;
/// `ls-files` reads only the index and enters no submodule.
fn gitlinks(dir: &Path) -> Result<BTreeSet<Vec<u8>>, ArchonError> {
    let args = ["ls-files", "--stage", "-z"];
    let output = git_output(dir, &args)?;
    if !output.status.success() {
        return Err(git_failure(dir, &args, &output));
    }
    let mut links = BTreeSet::new();
    // Each entry is `<mode> <object> <stage>\t<path>`.
    for entry in output.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let Some(tab) = entry.iter().position(|b| *b == b'\t') else {
            return Err(ArchonError::Invalid(format!(
                "git ls-files --stage printed a malformed entry in {}",
                dir.display()
            )));
        };
        if entry.starts_with(b"160000 ") {
            links.insert(entry[tab + 1..].to_vec());
        }
    }
    Ok(links)
}

fn lstat(path: &Path) -> Result<Option<std::fs::Metadata>, ArchonError> {
    match std::fs::symlink_metadata(path) {
        Ok(found) => Ok(Some(found)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ArchonError::io(format!("inspecting {}", path.display()), e)),
    }
}
