//! A role's worktree the daemon cannot move onto the next attempt's branch
//! is moved aside unentered instead, with its administrative git
//! directory, so the next attempt starts in a fresh worktree and the
//! operator still finds every file, commit and submodule git directory.
//! That is one holding submodules git must not enter, as saving what it
//! left would run that git, or one whose `HEAD` names no commit, as what
//! it left belongs to no commit. An item's run worktree is moved aside the
//! same way when the item is retried fresh from its base branch.

use std::path::{Path, PathBuf};

use archon_core::error::ArchonError;

use super::git_dirs::{apply_configs, file_name, plan_configs, rename, unused_path};
use super::identity::canonical;
use super::leftovers::keep_detached_head;
use super::submodules::Submodules;
use super::{listed, present};
use crate::worktree::{Head, path_arg};

/// Where worktrees are moved aside, under a team run's root. Stage ids and
/// roles never start with `.`, so no worktree is ever named so.
pub const SET_ASIDE_DIR: &str = ".set-aside";

/// Where their administrative directories are moved, in the repository's
/// common git directory: out of `worktrees/`, so git no longer lists them.
pub const GIT_SET_ASIDE_DIR: &str = "archon-set-aside";

/// A role's worktree, or an item's run worktree, the daemon moved aside,
/// running no git in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetAside {
    pub worktree: PathBuf,
    pub kept_at: PathBuf,
    /// Its administrative git directory: its index, reflogs and the git
    /// directories of submodules added in it with `git submodule add`.
    pub git_dir_kept_at: PathBuf,
    /// Where its commits stay: what it had checked out, or the name its
    /// detached `HEAD` was kept under.
    pub head: String,
    /// Its `HEAD` named a reference with no commit.
    pub unborn: bool,
    /// The submodules git must not enter, relative to the worktree.
    pub submodules: Vec<String>,
    /// The item's run worktree, moved aside for a fresh retry of the item.
    pub fresh_retry: bool,
}

impl SetAside {
    /// One line for the run's progress, with how to look at what was kept.
    pub fn describe(&self) -> String {
        let (whose, next) = match self.fresh_retry {
            true => (
                "the item's",
                "the new run starts in a fresh worktree off the base branch",
            ),
            false => ("a role's", "the new attempt starts in a fresh worktree"),
        };
        format!(
            "Moved {whose} worktree `{}` aside to `{}`, and its git directory (index, \
             reflogs, the git directories of submodules added in it) to `{}`, without \
             committing what it left: {}. {}; {next}. \
             The kept worktree has no `.git`, so no git runs in it by accident: to look at \
             it, first check the config of its submodules, then run `git --git-dir={} \
             --work-tree={} status`",
            self.worktree.display(),
            self.kept_at.display(),
            self.git_dir_kept_at.display(),
            self.why(),
            self.commits(),
            self.git_dir_kept_at.display(),
            self.kept_at.display()
        )
    }

    fn why(&self) -> String {
        if self.fresh_retry {
            return "the item was retried fresh from its base branch".to_owned();
        }
        let submodules: Vec<&str> = self.submodules.iter().map(String::as_str).collect();
        let foreign = (!submodules.is_empty()).then(|| {
            format!(
                "it holds submodules whose git directory the repository does not keep, \
                 which the daemon runs no git in: {}",
                listed(&submodules)
            )
        });
        let unborn = self.unborn.then(|| {
            format!(
                "its HEAD names {}, which has no commit, so what the role left there \
                 belongs to no commit",
                self.head
            )
        });
        let why: Vec<String> = unborn.into_iter().chain(foreign).collect();
        assert!(!why.is_empty(), "a worktree is set aside for a reason");
        why.join("; ")
    }

    fn commits(&self) -> String {
        match self.unborn {
            true => "Nothing it left was committed".to_owned(),
            false => format!("Its commits stay on {}", self.head),
        }
    }
}

/// Move the worktree `path`, proven registered with the administrative
/// directory `admin` in the common git directory `common`, aside under
/// [`SET_ASIDE_DIR`] beside it, and `admin` under [`GIT_SET_ASIDE_DIR`],
/// so a fresh worktree can be added at `path`. A detached `HEAD` is named
/// first, as gc no longer sees `admin` once moved.
pub(super) fn set_aside(
    common: &Path,
    admin: &Path,
    path: &Path,
    head: &Head,
    submodules: Submodules,
    unborn: bool,
    fresh_retry: bool,
) -> Result<SetAside, ArchonError> {
    assert!(path.is_absolute(), "worker worktrees are absolute");
    assert!(
        fresh_retry || unborn || !submodules.foreign.is_empty(),
        "only a worktree git must not enter, with no commit, or of a fresh retry is set aside"
    );
    assert_eq!(
        admin.parent(),
        Some(common.join("worktrees").as_path()),
        "check_worktree proved it a worktree's own"
    );
    let root = canonical(path)?;
    let mut gitfiles = own_modules(path, &root, admin, &submodules.kept)?;
    gitfiles.extend(foreign_gitfiles(path, &root, admin, &submodules.foreign)?);
    let configs = plan_configs(common, admin, Some(&root))?;
    let head = match head {
        Head::Detached => format!("`{}`", keep_detached_head(path)?),
        Head::Branch(_) | Head::Other(_) => head.to_string(),
    };
    let parent = path.parent().expect("worker paths have a parent");
    let kept_at = unused_path(&parent.join(SET_ASIDE_DIR), &file_name(path))?;
    let git_dir_kept_at = unused_path(&common.join(GIT_SET_ASIDE_DIR), &file_name(admin))?;
    tracing::warn!(worktree = %path.display(), kept_at = %kept_at.display(), git_dir = %git_dir_kept_at.display(), unborn, fresh_retry, submodules = ?submodules.foreign, "moving a worktree aside");
    rename(admin, &git_dir_kept_at)?;
    // Git reuses the moved directory's name for the fresh worktree: git run
    // where anything kept still named it would act on that one.
    apply_configs(common, &git_dir_kept_at, Some(&kept_at), &configs)?;
    repoint_gitfiles(path, &git_dir_kept_at, &gitfiles)?;
    remove_file(&path.join(".git"))?;
    rename(path, &kept_at)?;
    debug_assert!(!present(path)? && !present(admin)?, "both were moved");
    Ok(SetAside {
        worktree: path.to_owned(),
        kept_at,
        git_dir_kept_at,
        head,
        unborn,
        submodules: submodules.foreign,
        fresh_retry,
    })
}

/// A submodule of the worktree whose gitfile names a git directory in the
/// worktree's administrative directory.
struct Gitfile {
    /// Its work tree, relative to the worktree.
    work_tree: PathBuf,
    /// Its git directory, relative to the administrative directory.
    git_dir: PathBuf,
}

/// The `kept` submodules of the worktree `path` (canonical: `root`) whose
/// git directory is in its administrative directory `admin`, such as
/// those added with `git submodule add`. Each must be inside `path`.
fn own_modules(
    path: &Path,
    root: &Path,
    admin: &Path,
    kept: &[(String, PathBuf)],
) -> Result<Vec<Gitfile>, ArchonError> {
    let mut modules: Vec<Gitfile> = Vec::new();
    for (name, git_dir) in kept {
        let Ok(git_dir) = git_dir.strip_prefix(admin) else {
            continue;
        };
        let dir = canonical(&path.join(name))?;
        let work_tree = match dir.strip_prefix(root) {
            Ok(inside) if !inside.as_os_str().is_empty() => inside.to_owned(),
            _ => {
                return Err(ArchonError::Invalid(format!(
                    "submodule `{name}` of {} is at {}, outside it; the daemon does not \
                     move the worktree aside",
                    path.display(),
                    dir.display()
                )));
            }
        };
        // Two names can reach one directory through a symlink.
        if modules.iter().all(|module| module.work_tree != work_tree) {
            let git_dir = git_dir.to_owned();
            modules.push(Gitfile { work_tree, git_dir });
        }
    }
    debug_assert!(
        modules
            .iter()
            .all(|m| m.work_tree.is_relative() && m.git_dir.is_relative())
    );
    Ok(modules)
}

/// The `foreign` submodules of the worktree `path` (canonical: `root`)
/// whose gitfile still names a git directory in `admin`: read only, never
/// entered. A symlinked one, or one outside the worktree, is left alone.
fn foreign_gitfiles(
    path: &Path,
    root: &Path,
    admin: &Path,
    foreign: &[String],
) -> Result<Vec<Gitfile>, ArchonError> {
    assert!(
        root.is_absolute() && admin.is_absolute(),
        "both are canonical"
    );
    let mut found = Vec::new();
    for name in foreign {
        let dir = path.join(name);
        let Some(git_dir) = gitfile_target(&dir)? else {
            continue;
        };
        let Ok(git_dir) = git_dir.strip_prefix(admin) else {
            continue;
        };
        let inside = canonical(&dir)
            .ok()
            .and_then(|d| Some(d.strip_prefix(root).ok()?.to_owned()));
        let Some(work_tree) = inside.filter(|inside| !inside.as_os_str().is_empty()) else {
            tracing::warn!(submodule = %dir.display(), "a submodule outside the worktree names its git directory; left as it is");
            continue;
        };
        let git_dir = git_dir.to_owned();
        found.push(Gitfile { work_tree, git_dir });
    }
    debug_assert!(found.iter().all(|f| f.git_dir.is_relative()));
    Ok(found)
}

/// The canonical git directory the gitfile `dir/.git` names, when `dir` is
/// a real directory and its `.git` a plain file naming one that exists.
fn gitfile_target(dir: &Path) -> Result<Option<PathBuf>, ArchonError> {
    let is = |path: &Path, file: bool| match std::fs::symlink_metadata(path) {
        Ok(found) if file => Ok(found.file_type().is_file()),
        Ok(found) => Ok(found.file_type().is_dir()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(ArchonError::io(format!("inspecting {}", path.display()), e)),
    };
    let gitfile = dir.join(".git");
    if !is(dir, false)? || !is(&gitfile, true)? {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&gitfile)
        .map_err(|e| ArchonError::io(format!("reading {}", gitfile.display()), e))?;
    let Some(named) = text.lines().next().and_then(|l| l.strip_prefix("gitdir: ")) else {
        return Ok(None);
    };
    Ok(std::fs::canonicalize(dir.join(named)).ok())
}

/// Point each of `gitfiles` of the worktree `path`, whose git directories
/// moved with the administrative directory to `git_dir_kept_at`, there.
fn repoint_gitfiles(
    path: &Path,
    git_dir_kept_at: &Path,
    gitfiles: &[Gitfile],
) -> Result<(), ArchonError> {
    assert!(present(git_dir_kept_at)?, "the git directory was moved");
    assert!(present(path)?, "the worktree is still to be moved");
    for module in gitfiles {
        let git_dir = git_dir_kept_at.join(&module.git_dir);
        // Removed first: a symlink planted there is replaced, never followed.
        let gitfile = path.join(&module.work_tree).join(".git");
        remove_file(&gitfile)?;
        let text = format!("gitdir: {}\n", path_arg(&git_dir)?);
        std::fs::write(&gitfile, text)
            .map_err(|e| ArchonError::io(format!("writing {}", gitfile.display()), e))?;
    }
    Ok(())
}

fn remove_file(file: &Path) -> Result<(), ArchonError> {
    std::fs::remove_file(file)
        .map_err(|e| ArchonError::io(format!("removing {}", file.display()), e))
}
