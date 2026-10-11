//! The instance's own `voice.toml`: Aether's `/voice setup` refuses a link, so it
//! is a copy of the operator's, whose directory is never written.

use std::fs::{File, TryLockError};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rustix::fs::OFlags;

use super::same_dir;

mod publish;

pub const FILE: &str = "voice.toml";
/// Aether's installers hold this lock while they read and write `voice.toml`.
const LOCK: &str = ".voice-setup.lock";
const MODE: u32 = 0o600;

/// What the instance's `voice.toml` is, before anything is changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceFile {
    Absent,
    /// A regular file: the instance's own, never replaced.
    OwnFile,
    /// The link older setups made to the operator's file.
    OperatorLink,
    /// A link whose target is gone.
    Dangling(PathBuf),
    /// A live link to somewhere other than the operator's file.
    Elsewhere(PathBuf),
    /// A directory or another file type.
    NotAFile,
}

/// What seeding or healing did to the instance's `voice.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceOutcome {
    /// The instance had none; the operator's was copied.
    Copied,
    /// A link was replaced by a copy of the operator's file.
    Healed,
    /// A dangling link to this target was removed; nothing was there to copy.
    DanglingRemoved(PathBuf),
    /// Left exactly as found. `Absent` here means there was nothing to copy.
    Unchanged(VoiceFile),
    /// Left as found for now: a `/voice setup` holds the lock on the instance.
    SetupRunning(VoiceFile),
}

/// What `instance_dir/voice.toml` is relative to the operator's. Reads only.
pub fn inspect(operator_dir: &Path, instance_dir: &Path) -> anyhow::Result<VoiceFile> {
    let source = operator_dir.join(FILE);
    let dest = instance_dir.join(FILE);
    let meta = match std::fs::symlink_metadata(&dest) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(VoiceFile::Absent),
        Err(e) => return Err(anyhow::Error::new(e).context(format!("checking {}", dest.display()))),
    };
    if meta.is_file() {
        return Ok(VoiceFile::OwnFile);
    }
    if !meta.file_type().is_symlink() {
        return Ok(VoiceFile::NotAFile);
    }
    let target =
        std::fs::read_link(&dest).with_context(|| format!("reading {}", dest.display()))?;
    match std::fs::metadata(&dest) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(VoiceFile::Dangling(target));
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!("following {}", dest.display())));
        }
    }
    // Without a separate operator directory, no link is "the operator's".
    let operators = !same_dir(operator_dir, instance_dir) && same_file(&dest, &source);
    Ok(if operators {
        VoiceFile::OperatorLink
    } else {
        VoiceFile::Elsewhere(target)
    })
}

/// Whether two existing paths resolve to one file.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Replace a link to the operator's `voice.toml` by a copy of it, and remove
/// a dangling link. Idempotent; never creates a file where there was none.
pub fn heal(operator_dir: &Path, instance_dir: &Path) -> anyhow::Result<VoiceOutcome> {
    transition(operator_dir, instance_dir, false)
}

/// `heal`, then copy the operator's `voice.toml` where the instance has none.
pub fn seed(operator_dir: &Path, instance_dir: &Path) -> anyhow::Result<VoiceOutcome> {
    assert!(
        !same_dir(operator_dir, instance_dir),
        "seeding needs the operator's directory, not the instance's"
    );
    transition(operator_dir, instance_dir, true)
}

/// Inspect, and only where something must change take Aether's setup lock and
/// change it, so a running `/voice setup` never has the file moved under it.
fn transition(
    operator_dir: &Path,
    instance_dir: &Path,
    fill_absent: bool,
) -> anyhow::Result<VoiceOutcome> {
    assert!(
        operator_dir.is_absolute() && instance_dir.is_absolute(),
        "voice.toml is found by absolute path"
    );
    let found = inspect(operator_dir, instance_dir)?;
    if !needs_change(&found, &operator_dir.join(FILE), fill_absent)? {
        return Ok(VoiceOutcome::Unchanged(found));
    }
    let Some(lock) = lock_setup(instance_dir)? else {
        return Ok(VoiceOutcome::SetupRunning(found));
    };
    let outcome = change(operator_dir, instance_dir, fill_absent);
    let released = lock.unlock();
    let outcome = outcome?;
    released.with_context(|| format!("releasing {}", instance_dir.join(LOCK).display()))?;
    Ok(outcome)
}

fn needs_change(found: &VoiceFile, source: &Path, fill_absent: bool) -> anyhow::Result<bool> {
    match found {
        VoiceFile::OperatorLink | VoiceFile::Dangling(_) => Ok(true),
        VoiceFile::Absent if fill_absent => has_file(source),
        VoiceFile::Absent | VoiceFile::OwnFile | VoiceFile::Elsewhere(_) | VoiceFile::NotAFile => {
            Ok(false)
        }
    }
}

fn has_file(path: &Path) -> anyhow::Result<bool> {
    path.try_exists()
        .with_context(|| format!("checking {}", path.display()))
}

/// Aether's voice setup lock on `instance_dir`, or `None` while a setup holds it.
pub(super) fn lock_setup(instance_dir: &Path) -> anyhow::Result<Option<File>> {
    let path = instance_dir.join(LOCK);
    let flags = (OFlags::NOFOLLOW | OFlags::NONBLOCK).bits();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(MODE)
        .custom_flags(i32::try_from(flags).context("open flags")?)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    let meta = file
        .metadata()
        .with_context(|| format!("checking {}", path.display()))?;
    anyhow::ensure!(meta.is_file(), "{} is not a regular file", path.display());
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => {
            Err(anyhow::Error::new(e).context(format!("locking {}", path.display())))
        }
    }
}

/// Under the lock: look again, then change only what is still as it was found.
fn change(
    operator_dir: &Path,
    instance_dir: &Path,
    fill_absent: bool,
) -> anyhow::Result<VoiceOutcome> {
    let source = operator_dir.join(FILE);
    let dest = instance_dir.join(FILE);
    let fill = fill_absent && has_file(&source)?;
    let changed = match inspect(operator_dir, instance_dir)? {
        VoiceFile::OperatorLink => {
            publish::replace_link(&source, &dest)?.then_some(VoiceOutcome::Healed)
        }
        VoiceFile::Dangling(target) => {
            let removed = publish::remove_dangling(&dest)?;
            match removed && fill {
                true => publish::copy_new(&source, &dest)?.then_some(VoiceOutcome::Healed),
                false => removed.then_some(VoiceOutcome::DanglingRemoved(target)),
            }
        }
        VoiceFile::Absent if fill => {
            publish::copy_new(&source, &dest)?.then_some(VoiceOutcome::Copied)
        }
        _ => None,
    };
    match changed {
        Some(outcome) => settled(outcome, operator_dir, instance_dir),
        // It changed while this ran: whatever is there now is left alone.
        None => Ok(VoiceOutcome::Unchanged(inspect(
            operator_dir,
            instance_dir,
        )?)),
    }
}

/// `outcome`, unless a published copy has already been replaced by something
/// else: the name is public, so that is reported as found and never reset.
pub(super) fn settled(
    outcome: VoiceOutcome,
    operator_dir: &Path,
    instance_dir: &Path,
) -> anyhow::Result<VoiceOutcome> {
    assert!(
        !matches!(
            outcome,
            VoiceOutcome::Unchanged(_) | VoiceOutcome::SetupRunning(_)
        ),
        "only a change is settled: {outcome:?}"
    );
    if matches!(outcome, VoiceOutcome::DanglingRemoved(_)) {
        return Ok(outcome);
    }
    match inspect(operator_dir, instance_dir)? {
        VoiceFile::OwnFile => Ok(outcome),
        found => Ok(VoiceOutcome::Unchanged(found)),
    }
}
