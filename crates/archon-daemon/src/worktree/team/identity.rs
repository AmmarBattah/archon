//! Which checkout a directory in a team run really is. A role controls
//! its worktree, `.git` file included, so before the daemon runs git
//! there it proves the directory is the worktree git registered at that
//! path: otherwise its commits and switches could land in the operator's
//! checkout, or in another worktree of the repository.

use std::path::{Path, PathBuf};

use archon_core::error::ArchonError;

use super::git;

/// Refuse `path` unless it is a registered worktree of the repository
/// whose common git directory is `common`, and git reaches that worktree's
/// own administrative directory from it: never the repository's own, nor
/// another worktree's. `path` and its `.git` file are never symlinks.
/// Returns that administrative directory, canonical.
pub(crate) fn check_worktree(path: &Path, common: &Path) -> Result<PathBuf, ArchonError> {
    assert!(path.is_absolute(), "team worktrees are absolute");
    assert!(common.is_absolute(), "common git directories are canonical");
    if lstat(path)?.file_type().is_symlink() {
        return Err(not_registered(path, "it is a symbolic link"));
    }
    let dot_git = match std::fs::symlink_metadata(path.join(".git")) {
        Ok(found) => found,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ArchonError::Invalid(format!(
                "{} is not a git worktree: it has no `.git`",
                path.display()
            )));
        }
        Err(e) => return Err(ArchonError::io(format!("inspecting {}", path.display()), e)),
    };
    if !dot_git.file_type().is_file() {
        return Err(not_registered(path, "its `.git` is not a plain gitfile"));
    }
    let located = locate(path)?;
    let own = canonical(path)?;
    if located.top != own {
        return Err(ArchonError::Invalid(format!(
            "{} is not a git worktree (git found the work tree {} around it)",
            path.display(),
            located.top.display()
        )));
    }
    let found = located.common;
    if found != common {
        return Err(ArchonError::Invalid(format!(
            "{} is a worktree of {}, not of the item's repository ({})",
            path.display(),
            found.display(),
            common.display()
        )));
    }
    let admin = located.git_dir;
    if admin.parent() != Some(common.join("worktrees").as_path()) {
        let why = format!(
            "git reaches {} from it, not a worktree's own",
            admin.display()
        );
        return Err(not_registered(path, &why));
    }
    let registered = registered_at(&admin)?;
    let expected = own.join(".git");
    if registered != expected {
        let why = format!("{} belongs to {}", admin.display(), registered.display());
        return Err(not_registered(path, &why));
    }
    Ok(admin)
}

/// The canonical git directory `tree`'s repository shares across worktrees.
pub(crate) fn common_dir(tree: &Path) -> Result<PathBuf, ArchonError> {
    let args = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
    let dir = git(tree, &args)?;
    canonical(Path::new(&dir))
}

/// Where git finds a checkout from `tree`, each canonical.
struct Located {
    /// The top level of its work tree.
    top: PathBuf,
    /// The git directory its repository shares across worktrees.
    common: PathBuf,
    /// Its own git directory.
    git_dir: PathBuf,
}

/// [`Located`] for `tree`, from one `rev-parse`.
fn locate(tree: &Path) -> Result<Located, ArchonError> {
    let args = [
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-common-dir",
        "--git-dir",
    ];
    let printed = git(tree, &args)?;
    let lines: Vec<&str> = printed.lines().collect();
    // A path holding a newline would print more lines: nothing to trust.
    let [top, common, git_dir] = lines.as_slice() else {
        return Err(ArchonError::Invalid(format!(
            "git rev-parse printed {} lines for {}, not 3",
            lines.len(),
            tree.display()
        )));
    };
    let located = Located {
        top: canonical(Path::new(top))?,
        common: canonical(Path::new(common))?,
        git_dir: canonical(Path::new(git_dir))?,
    };
    Ok(located)
}

/// The `.git` file the worktree administrative directory `admin` was
/// registered for, as git wrote it into `admin/gitdir`: absolute, or
/// relative to `admin` (`worktree.useRelativePaths`). Its last component
/// is not resolved, so a symlinked `.git` never matches.
fn registered_at(admin: &Path) -> Result<PathBuf, ArchonError> {
    let file = admin.join("gitdir");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| ArchonError::io(format!("reading {}", file.display()), e))?;
    let written = Path::new(text.trim_end_matches(['\n', '\r']));
    let (Some(dir), Some(name)) = (written.parent(), written.file_name()) else {
        return Err(ArchonError::Invalid(format!(
            "{} names no `.git` file",
            file.display()
        )));
    };
    let dir = match dir.is_absolute() {
        true => dir.to_owned(),
        false => admin.join(dir),
    };
    let resolved = canonical(&dir)?.join(name);
    debug_assert!(resolved.is_absolute());
    Ok(resolved)
}

fn lstat(path: &Path) -> Result<std::fs::Metadata, ArchonError> {
    std::fs::symlink_metadata(path)
        .map_err(|e| ArchonError::io(format!("inspecting {}", path.display()), e))
}

pub(super) fn canonical(path: &Path) -> Result<PathBuf, ArchonError> {
    std::fs::canonicalize(path)
        .map_err(|e| ArchonError::io(format!("resolving {}", path.display()), e))
}

fn not_registered(path: &Path, why: &str) -> ArchonError {
    ArchonError::Invalid(format!(
        "{} is not the worktree git registered there: {why}; the daemon runs no git in it",
        path.display()
    ))
}
