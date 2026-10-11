//! Changes to the instance's `voice.toml` that never replace what they did not
//! inspect: the old link is moved aside whole and the copy is published by link.

use std::io::ErrorKind;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rustix::fs::{CWD, RenameFlags, renameat_with};

use super::{FILE, MODE, same_file};

/// The link taken from `dest`, inside the scratch directory.
const HELD: &str = "held";
const STAGED: &str = "staged";

/// Replace the link at `dest` by a copy of `source`. False, and nothing lost,
/// when `dest` is no longer that link or a file appeared in its place.
pub fn replace_link(source: &Path, dest: &Path) -> anyhow::Result<bool> {
    let dir = parent(dest)?;
    in_scratch(dir, |scratch| {
        let staged = stage(source, scratch)?;
        let held = scratch.join(HELD);
        if !take_link(dest, &held, |target| same_file(&dir.join(target), source))? {
            return Ok(false);
        }
        match publish(&staged, dest) {
            Ok(published) => Ok(published),
            Err(error) => {
                put_back(&held, dest, false)?;
                Err(error)
            }
        }
    })
}

/// Remove the dangling link at `dest`. False when it is no longer one.
pub fn remove_dangling(dest: &Path) -> anyhow::Result<bool> {
    let dir = parent(dest)?;
    in_scratch(dir, |scratch| {
        take_link(
            dest,
            &scratch.join(HELD),
            |target| matches!(std::fs::metadata(dir.join(target)), Err(e) if e.kind() == ErrorKind::NotFound),
        )
    })
}

/// Copy `source` to `dest` where nothing is. False when something is there.
pub fn copy_new(source: &Path, dest: &Path) -> anyhow::Result<bool> {
    in_scratch(parent(dest)?, |scratch| {
        publish(&stage(source, scratch)?, dest)
    })
}

fn parent(dest: &Path) -> anyhow::Result<&Path> {
    assert_eq!(dest.file_name(), Some(FILE.as_ref()), "only voice.toml");
    dest.parent().context("voice.toml has no directory")
}

/// Run `work` in a new private directory in `dir`, removed afterwards unless
/// `work` failed while it still held what it took from `dir`.
fn in_scratch<T>(dir: &Path, work: impl FnOnce(&Path) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let scratch = tempfile::Builder::new()
        .prefix(".voice.toml.")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(dir)
        .with_context(|| format!("creating a temporary directory in {}", dir.display()))?;
    let result = work(scratch.path());
    let holds = std::fs::symlink_metadata(scratch.path().join(HELD)).is_ok();
    if result.is_err() && holds {
        let kept = scratch.keep();
        return result.with_context(|| format!("{} is kept, not removed", kept.display()));
    }
    let path = scratch.path().to_owned();
    let removed = scratch.close();
    let value = result?;
    removed.with_context(|| format!("removing {}", path.display()))?;
    Ok(value)
}

/// The bytes of `source` as a private regular file in `scratch`, synced.
pub(super) fn stage(source: &Path, scratch: &Path) -> anyhow::Result<PathBuf> {
    assert_ne!(
        source.parent(),
        Some(scratch),
        "the operator's is not written"
    );
    let meta =
        std::fs::metadata(source).with_context(|| format!("checking {}", source.display()))?;
    anyhow::ensure!(
        meta.is_file(),
        "{} is not a regular file; not copying it",
        source.display()
    );
    let bytes = std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    let staged = scratch.join(STAGED);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(MODE)
        .open(&staged)
        .with_context(|| format!("creating {}", staged.display()))?;
    // The umask can only clear bits, so set the mode outright.
    file.set_permissions(std::fs::Permissions::from_mode(MODE))
        .and_then(|()| file.write_all(&bytes))
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", staged.display()))?;
    // Checked on the private file: once published, the name is anyone's to change.
    let meta = file
        .metadata()
        .with_context(|| format!("checking {}", staged.display()))?;
    assert!(meta.is_file(), "Aether requires a regular voice.toml");
    assert_eq!(meta.permissions().mode() & 0o777, MODE, "voice.toml mode");
    Ok(staged)
}

/// Give `staged` the name `dest` unless something has it: a hard link never
/// replaces. False, and `dest` untouched, when something is there.
pub(super) fn publish(staged: &Path, dest: &Path) -> anyhow::Result<bool> {
    assert_eq!(
        staged.file_name(),
        Some(STAGED.as_ref()),
        "only a staged copy"
    );
    assert_ne!(
        staged.parent(),
        dest.parent(),
        "staged in the scratch directory"
    );
    match std::fs::hard_link(staged, dest) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(anyhow::Error::new(e).context(format!("creating {}", dest.display()))),
    }
}

/// Move whatever is at `dest` to `held` in one step, and keep it only if it is
/// a link whose target `wanted` accepts; anything else is put back. True if kept.
pub(super) fn take_link(
    dest: &Path,
    held: &Path,
    wanted: impl FnOnce(&Path) -> bool,
) -> anyhow::Result<bool> {
    assert_ne!(
        dest.parent(),
        held.parent(),
        "held in the scratch directory"
    );
    match std::fs::rename(dest, held) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!("moving {}", dest.display())));
        }
    }
    let meta =
        std::fs::symlink_metadata(held).with_context(|| format!("checking {}", held.display()))?;
    if meta.file_type().is_symlink() {
        let target =
            std::fs::read_link(held).with_context(|| format!("reading {}", held.display()))?;
        if wanted(&target) {
            return Ok(true);
        }
    }
    put_back(held, dest, meta.is_dir())?;
    Ok(false)
}

/// Return `held` to `dest` without replacing anything that appeared there:
/// if something did, `held` stays where it is and the error says so.
pub(super) fn put_back(held: &Path, dest: &Path, is_dir: bool) -> anyhow::Result<()> {
    assert_ne!(
        held.parent(),
        dest.parent(),
        "held in the scratch directory"
    );
    // A directory cannot be hard-linked, and a plain rename replaces an empty one.
    let restored = if is_dir {
        renameat_with(CWD, held, CWD, dest, RenameFlags::NOREPLACE).map_err(std::io::Error::from)
    } else {
        std::fs::hard_link(held, dest).and_then(|()| std::fs::remove_file(held))
    };
    restored.with_context(|| {
        format!(
            "{} was moved to {} and could not be put back; move it back by hand",
            dest.display(),
            held.display()
        )
    })
}

#[cfg(test)]
mod tests;
