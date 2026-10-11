//! Moving a worktree's administrative git directory out of `worktrees/`
//! while running no git in it: where it goes, and the `core.worktree` of
//! each submodule git directory in it, which named the old work trees.

use std::path::{Path, PathBuf};

use archon_core::error::ArchonError;

use super::{git, git_output, present};
use crate::worktree::{git_failure, path_arg};

/// Most worktrees or git directories of one name kept aside at once.
const MAX_KEPT: usize = 1000;

/// Most directories searched for submodule git directories in one
/// administrative directory: a role could create any number.
const MAX_MODULE_DIRS: usize = 10_000;

/// A submodule git directory's `core.worktree`, fixed once it has moved.
#[derive(Debug)]
pub(super) struct ConfigFix {
    /// The submodule git directory, relative to the administrative one.
    git_dir: PathBuf,
    /// Where its work tree is, relative to the worktree; `None` when that
    /// is outside the worktree or gone, and the setting is removed.
    work_tree: Option<PathBuf>,
}

/// The `core.worktree` settings of every submodule git directory under
/// `admin/modules`, read before `admin` moves. Those naming a directory
/// inside the worktree `root` are repointed; any other is removed, so git
/// never acts on a checkout from a git directory kept aside.
pub(super) fn plan_configs(
    common: &Path,
    admin: &Path,
    root: Option<&Path>,
) -> Result<Vec<ConfigFix>, ArchonError> {
    assert!(
        admin.is_absolute(),
        "administrative directories are canonical"
    );
    assert!(
        root.is_none_or(Path::is_absolute),
        "worktrees are canonical"
    );
    let mut fixes = Vec::new();
    for git_dir in module_git_dirs(admin)? {
        let config = admin.join(&git_dir).join("config");
        let Some(value) = config_worktree(common, &config)? else {
            continue;
        };
        // An absolute value replaces the base; a relative one is from the git directory.
        let named = admin.join(&git_dir).join(&value);
        let work_tree = std::fs::canonicalize(&named).ok().and_then(|found| {
            let inside = found.strip_prefix(root?).ok()?;
            Some(inside.to_owned())
        });
        fixes.push(ConfigFix { git_dir, work_tree });
    }
    Ok(fixes)
}

/// Apply `fixes` to the git directories now under `moved`: each work tree
/// repointed to where it is kept under `kept_at`, or its setting removed.
/// Git runs in the trusted common git directory `common`, reading only
/// the file it is given.
pub(super) fn apply_configs(
    common: &Path,
    moved: &Path,
    kept_at: Option<&Path>,
    fixes: &[ConfigFix],
) -> Result<(), ArchonError> {
    assert!(present(moved)?, "the git directory was moved");
    if let Some(kept_at) = kept_at {
        assert!(
            !present(kept_at)?,
            "the worktree moves after its git directories"
        );
    }
    for fix in fixes {
        let config = moved.join(&fix.git_dir).join("config");
        let config = path_arg(&config)?;
        match (&fix.work_tree, kept_at) {
            (Some(inside), Some(kept_at)) => {
                let work_tree = kept_at.join(inside);
                let args = [
                    "config",
                    "--file",
                    config,
                    "core.worktree",
                    path_arg(&work_tree)?,
                ];
                git(common, &args)?;
            }
            _ => {
                git(
                    common,
                    &["config", "--file", config, "--unset-all", "core.worktree"],
                )?;
            }
        }
    }
    Ok(())
}

/// `core.worktree` in the config file `config`, if set.
fn config_worktree(common: &Path, config: &Path) -> Result<Option<String>, ArchonError> {
    let args = [
        "config",
        "--file",
        path_arg(config)?,
        "--get",
        "core.worktree",
    ];
    let output = git_output(common, &args)?;
    // `git config --get` exits 1, silently, when the key is not set.
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(None),
        _ => return Err(git_failure(common, &args, &output)),
    }
    let value = String::from_utf8(output.stdout).map_err(|_| {
        ArchonError::Invalid(format!(
            "core.worktree in {} is not UTF-8",
            config.display()
        ))
    })?;
    let value = value.trim_end_matches('\n');
    assert!(!value.contains('\n'), "--get prints one value");
    Ok(Some(value.to_owned()))
}

/// Every submodule git directory under `admin/modules`, at any depth and
/// relative to `admin`. A submodule's name may hold `/`, so directories
/// are searched until one is a git directory; then only its `modules`.
/// Symlinks are never followed.
fn module_git_dirs(admin: &Path) -> Result<Vec<PathBuf>, ArchonError> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::from("modules")];
    let mut searched = 0;
    while let Some(relative) = pending.pop() {
        searched += 1;
        if searched > MAX_MODULE_DIRS {
            return Err(ArchonError::Invalid(format!(
                "{} holds more than {MAX_MODULE_DIRS} directories under `modules`; the \
                 daemon does not move it",
                admin.display()
            )));
        }
        let dir = admin.join(&relative);
        if !real_dir(&dir)? {
            continue;
        }
        if is_file(&dir.join("config"))? && is_file(&dir.join("HEAD"))? {
            pending.push(relative.join("modules"));
            found.push(relative);
            continue;
        }
        let reading = |e| ArchonError::io(format!("reading {}", dir.display()), e);
        for entry in std::fs::read_dir(&dir).map_err(reading)? {
            let entry = entry.map_err(reading)?;
            if entry.file_type().map_err(reading)?.is_dir() {
                pending.push(relative.join(entry.file_name()));
            }
        }
    }
    debug_assert!(found.iter().all(|dir| dir.starts_with("modules")));
    Ok(found)
}

/// The first `<dir>/<name>-<n>` nothing is at, creating `dir`, which must
/// be a real directory: never a symlink leading elsewhere.
pub(super) fn unused_path(dir: &Path, name: &str) -> Result<PathBuf, ArchonError> {
    assert!(!name.is_empty(), "worktrees have names");
    std::fs::create_dir_all(dir)
        .map_err(|e| ArchonError::io(format!("creating {}", dir.display()), e))?;
    if !real_dir(dir)? {
        return Err(ArchonError::Invalid(format!(
            "{} is not a directory but a symbolic link or file; the daemon moves nothing \
             there. Remove it",
            dir.display()
        )));
    }
    for n in 1..=MAX_KEPT {
        let candidate = dir.join(format!("{name}-{n}"));
        if !present(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(ArchonError::Invalid(format!(
        "{} already holds {MAX_KEPT} worktrees moved aside for {name}; remove them",
        dir.display()
    )))
}

/// Whether `path` is a directory itself, not a symlink to one.
fn real_dir(path: &Path) -> Result<bool, ArchonError> {
    Ok(lstat(path)?.is_some_and(|found| found.file_type().is_dir()))
}

fn is_file(path: &Path) -> Result<bool, ArchonError> {
    Ok(lstat(path)?.is_some_and(|found| found.file_type().is_file()))
}

fn lstat(path: &Path) -> Result<Option<std::fs::Metadata>, ArchonError> {
    match std::fs::symlink_metadata(path) {
        Ok(found) => Ok(Some(found)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ArchonError::io(format!("inspecting {}", path.display()), e)),
    }
}

pub(super) fn file_name(path: &Path) -> String {
    let name = path.file_name().expect("worktree paths name a directory");
    name.to_string_lossy().into_owned()
}

pub(super) fn rename(from: &Path, to: &Path) -> Result<(), ArchonError> {
    std::fs::rename(from, to).map_err(|e| {
        let what = format!("moving {} to {}", from.display(), to.display());
        ArchonError::io(what, e)
    })
}
