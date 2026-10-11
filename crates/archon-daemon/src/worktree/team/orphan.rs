//! A role's worktree deleted by hand stays registered in git, which blocks
//! adding a worktree there. `git worktree remove` would delete its
//! administrative directory, and with it the git directories of submodules
//! added in it; the daemon moves that directory aside instead.

use std::path::{Component, Path, PathBuf};

use archon_core::error::ArchonError;

use super::git_dirs::{apply_configs, file_name, plan_configs, rename, unused_path};
use super::leftovers::keep_commit;
use super::{GIT_SET_ASIDE_DIR, present};

/// The administrative git directory of a role's worktree deleted by hand,
/// moved aside rather than deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanedGitDir {
    pub worktree: PathBuf,
    pub git_dir_kept_at: PathBuf,
    /// Where its commits stay: what it had checked out, or the name its
    /// detached `HEAD` was kept under.
    pub head: String,
}

impl OrphanedGitDir {
    /// One line for the run's progress.
    pub fn describe(&self) -> String {
        format!(
            "A role's worktree `{}` was deleted by hand; moved its git directory (index, \
             reflogs, the git directories of submodules added in it) aside to `{}` rather \
             than deleting it. Its commits stay on {}; the new attempt starts in a fresh \
             worktree",
            self.worktree.display(),
            self.git_dir_kept_at.display(),
            self.head
        )
    }
}

/// Move the administrative directory of the missing worktree `path` out
/// of `<common>/worktrees/` into [`GIT_SET_ASIDE_DIR`], so git no longer
/// lists it; `None` when git has none registered there. A detached `HEAD`
/// is named first, as gc no longer sees the directory once moved.
pub(super) fn set_aside_orphan(
    repo_path: &Path,
    common: &Path,
    path: &Path,
) -> Result<Option<OrphanedGitDir>, ArchonError> {
    assert!(path.is_absolute(), "worker worktrees are absolute");
    assert!(!present(path)?, "only a missing worktree is orphaned");
    let Some(admin) = registered_admin(common, path)? else {
        return Ok(None);
    };
    let head = keep_head(repo_path, &admin)?;
    // Its submodules' work trees are gone with the worktree: none is kept.
    let configs = plan_configs(common, &admin, None)?;
    let kept = unused_path(&common.join(GIT_SET_ASIDE_DIR), &file_name(&admin))?;
    tracing::warn!(worktree = %path.display(), git_dir = %kept.display(), "a role's worktree was deleted by hand; moving its git directory aside");
    rename(&admin, &kept)?;
    apply_configs(common, &kept, None, &configs)?;
    debug_assert!(!present(&admin)?, "it was moved");
    Ok(Some(OrphanedGitDir {
        worktree: path.to_owned(),
        git_dir_kept_at: kept,
        head,
    }))
}

/// The administrative directory in `<common>/worktrees/` registered for a
/// worktree at `path`, by the `gitdir` file git wrote there. Two would be
/// a repository git itself refuses to add to.
fn registered_admin(common: &Path, path: &Path) -> Result<Option<PathBuf>, ArchonError> {
    let parent = path.parent().expect("worker paths have a parent");
    let parent = std::fs::canonicalize(parent)
        .map_err(|e| ArchonError::io(format!("resolving {}", parent.display()), e))?;
    let name = path.file_name().expect("worker paths name a directory");
    let wanted = [path.join(".git"), parent.join(name).join(".git")];
    let worktrees = common.join("worktrees");
    let reading = |e| ArchonError::io(format!("reading {}", worktrees.display()), e);
    let entries = match std::fs::read_dir(&worktrees) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(reading(e)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(reading)?;
        if !entry.file_type().map_err(reading)?.is_dir() {
            continue;
        }
        let admin = entry.path();
        let Some(registered) = registered_for(&admin)? else {
            continue;
        };
        if wanted.contains(&registered) {
            found.push(admin);
        }
    }
    if found.len() > 1 {
        return Err(ArchonError::Invalid(format!(
            "git has {} worktrees registered at {}; run `git worktree prune` there",
            found.len(),
            path.display()
        )));
    }
    Ok(found.pop())
}

/// The `.git` file `admin` was registered for, from `admin/gitdir`:
/// absolute, or relative to `admin`, normalised without the filesystem.
fn registered_for(admin: &Path) -> Result<Option<PathBuf>, ArchonError> {
    let file = admin.join("gitdir");
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ArchonError::io(format!("reading {}", file.display()), e)),
    };
    let written = Path::new(text.trim_end_matches(['\n', '\r']));
    let mut normal = PathBuf::new();
    for part in admin.join(written).components() {
        match part {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    debug_assert!(normal.is_absolute(), "admin is absolute");
    Ok(Some(normal))
}

/// What the orphaned `admin` had checked out: a reference by name, or a
/// detached commit kept under `refs/archon/saved/` in `repo_path`.
fn keep_head(repo_path: &Path, admin: &Path) -> Result<String, ArchonError> {
    let file = admin.join("HEAD");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| ArchonError::io(format!("reading {}", file.display()), e))?;
    let text = text.trim_end_matches(['\n', '\r']);
    if let Some(reference) = text.strip_prefix("ref: ") {
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        return Ok(format!("`{name}`"));
    }
    if text.len() < 40 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ArchonError::Invalid(format!(
            "{} names neither a reference nor a commit",
            file.display()
        )));
    }
    Ok(format!("`{}`", keep_commit(repo_path, text)?))
}
