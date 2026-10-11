//! Bounded report snapshots. Directory descriptors and O_NOFOLLOW keep a
//! source changed during copying from redirecting reads outside the tree.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use archon_core::Timestamp;
use archon_core::error::ArchonError;
use archon_core::id::ReportId;
use rustix::fs::{CWD, Dir, Mode, OFlags, RenameFlags, open, openat, renameat_with};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_DEPTH: usize = 32;

/// Open an absolute directory through real directories only, refusing `..`.
fn directory(path: &Path) -> std::io::Result<File> {
    if !path.is_absolute() {
        return Err(std::io::Error::other("report directories must be absolute"));
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut fd = open("/", flags, Mode::empty())?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => fd = openat(&fd, name, flags, Mode::empty())?,
            _ => {
                return Err(std::io::Error::other(
                    "report paths must be absolute without '..'",
                ));
            }
        }
    }
    Ok(File::from(fd))
}

pub(super) fn validate_path(html: &Path) -> Result<(), ArchonError> {
    let parent = html
        .parent()
        .ok_or_else(|| ArchonError::Invalid("report needs a directory".into()))?;
    let dir = directory(parent)
        .map_err(|e| ArchonError::io("opening report directory (no symlinks or '..')", e))?;
    let name = html
        .file_name()
        .ok_or_else(|| ArchonError::Invalid("report needs a file name".into()))?;
    let fd = openat(
        &dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| ArchonError::io("opening report (no symlinks)", e.into()))?;
    if !File::from(fd)
        .metadata()
        .map_err(|e| ArchonError::io("reading report metadata", e))?
        .is_file()
    {
        return Err(ArchonError::Invalid("report must be a regular file".into()));
    }
    Ok(())
}

/// An installed copy remains reversible until its record has been written.
/// Cancellation rolls back too; crashes leave entries for orphan pruning.
pub(super) struct Snapshot {
    pub html: PathBuf,
    root: PathBuf,
    report: ReportId,
    backup: Option<PathBuf>,
    first_publication: bool,
    committed: bool,
}

impl Snapshot {
    pub fn commit(&mut self) -> Result<(), ArchonError> {
        // Once recorded, the new directory must survive even if backup cleanup fails.
        self.committed = true;
        if let Some(backup) = &self.backup {
            fs::remove_dir_all(backup)
                .map_err(|e| ArchonError::io("removing previous report snapshot", e))?;
        }
        Ok(())
    }

    pub fn abort(&mut self) -> Result<(), ArchonError> {
        if self.committed {
            return Ok(());
        }
        self.committed = true;
        self.rollback()
    }

    fn rollback(&self) -> Result<(), ArchonError> {
        let target = self.root.join(self.report.as_str());
        if let Some(backup) = &self.backup {
            renameat_with(CWD, backup, CWD, &target, RenameFlags::EXCHANGE)
                .map_err(|e| ArchonError::io("restoring previous report snapshot", e.into()))?;
            File::open(&self.root)
                .and_then(|root| root.sync_all())
                .map_err(|e| ArchonError::io("syncing restored report snapshot", e))?;
            fs::remove_dir_all(backup)
                .map_err(|e| ArchonError::io("removing failed report refresh", e))
        } else if self.first_publication {
            remove(&self.root, &self.report)
        } else {
            // No previous copy to restore (a legacy or fallback record). Leave
            // this id's directory intact; unreferenced copies age out in prune.
            Ok(())
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if !self.committed
            && let Err(error) = self.rollback()
        {
            tracing::warn!(%error, report = %self.report, "report snapshot rollback failed");
        }
    }
}

/// Build before replacing. Keep the previous directory for rollback until
/// publication records the new copy. Linux exchange keeps the served path present.
pub(super) fn snapshot(
    root: &Path,
    report: &ReportId,
    html: &Path,
    first_publication: bool,
) -> Result<Snapshot, ArchonError> {
    validate_path(html)?;
    let target = root.join(report.as_str());
    let staged = root.join(format!(".{}-{}", report, ReportId::new()));
    let source = html.parent().expect("validated HTML has a parent");
    let canonical =
        fs::canonicalize(html).map_err(|e| ArchonError::io("resolving snapshot source", e))?;
    if canonical != html.components().collect::<PathBuf>() {
        return Err(ArchonError::Invalid(
            "report path resolves through a symlink or '..'".into(),
        ));
    }
    if root.starts_with(source) {
        return Err(ArchonError::Invalid(
            "report directory contains the report archive".into(),
        ));
    }
    let result = (|| -> std::io::Result<Snapshot> {
        fs::create_dir_all(root)?;
        directory(root)?;
        fs::create_dir(&staged)?;
        let mut budget = Budget {
            bytes: MAX_BYTES,
            entries: MAX_ENTRIES,
        };
        copy(&directory(source)?, &staged, 0, &mut budget)?;
        let copied_html = staged.join(html.file_name().expect("validated file name"));
        if !fs::metadata(copied_html)?.is_file() {
            return Err(std::io::Error::other("snapshot is missing its HTML file"));
        }
        let backup = match fs::symlink_metadata(&target) {
            Ok(_) => {
                directory(&target)?;
                renameat_with(CWD, &staged, CWD, &target, RenameFlags::EXCHANGE)?;
                Some(staged.clone())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::rename(&staged, &target)?;
                None
            }
            Err(e) => return Err(e),
        };
        let snapshot = Snapshot {
            html: target.join(html.file_name().expect("validated file name")),
            root: root.to_owned(),
            report: report.clone(),
            backup,
            first_publication,
            committed: false,
        };
        Ok(snapshot)
    })();
    match result {
        Ok(mut snapshot) => {
            if let Err(error) = File::open(root).and_then(|dir| dir.sync_all()) {
                snapshot.abort().map_err(|cleanup| {
                    cleanup.with_detail(&format!("syncing report archive: {error}"))
                })?;
                return Err(ArchonError::io("syncing report archive", error));
            }
            Ok(snapshot)
        }
        Err(error) => {
            match fs::remove_dir_all(&staged) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(cleanup) => {
                    return Err(ArchonError::io(
                        format!("copying report: {error}; cleaning incomplete snapshot"),
                        cleanup,
                    ));
                }
            }
            Err(ArchonError::io("copying report snapshot", error))
        }
    }
}

struct Budget {
    bytes: u64,
    entries: usize,
}

fn copy(source: &File, target: &Path, depth: usize, budget: &mut Budget) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(std::io::Error::other(
            "report exceeds directory depth limit",
        ));
    }
    for entry in Dir::read_from(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        budget.entries = budget
            .entries
            .checked_sub(1)
            .ok_or_else(|| std::io::Error::other("report exceeds entry limit"))?;
        let fd = openat(
            source,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let mut file = File::from(fd);
        let metadata = file.metadata()?;
        let dest = target.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        if metadata.is_dir() {
            fs::create_dir(&dest)?;
            copy(&file, &dest, depth + 1, budget)?;
        } else if metadata.is_file() {
            if metadata.len() > budget.bytes {
                return Err(std::io::Error::other("report exceeds 64 MiB limit"));
            }
            let mut output = File::create(&dest)?;
            let copied = std::io::copy(&mut (&mut file).take(budget.bytes + 1), &mut output)?;
            budget.bytes = budget
                .bytes
                .checked_sub(copied)
                .ok_or_else(|| std::io::Error::other("report exceeds 64 MiB limit"))?;
            output.flush()?;
            output.sync_all()?;
        } else {
            return Err(std::io::Error::other("report contains a special file"));
        }
    }
    File::open(target)?.sync_all()
}

/// Delete only this id's real snapshot directory; never the source file.
pub(super) fn remove(root: &Path, report: &ReportId) -> Result<(), ArchonError> {
    let path = root.join(report.as_str());
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ArchonError::io("reading report snapshot", e)),
        Ok(meta) if !meta.is_dir() => {
            return Err(ArchonError::Invalid(format!(
                "{} is not a real report directory",
                path.display()
            )));
        }
        Ok(_) => {}
    }
    directory(root).map_err(|e| ArchonError::io("opening report archive", e))?;
    fs::remove_dir_all(&path).map_err(|e| ArchonError::io("removing report snapshot", e))
}

/// Reclaim only Archon's unreferenced directories older than retention.
/// The caller holds report_files through the record scan and this sweep.
pub(super) fn prune_orphans(
    root: &Path,
    referenced: &BTreeSet<PathBuf>,
    cutoff: Timestamp,
) -> Result<(), ArchonError> {
    match directory(root) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ArchonError::io("opening report archive", e)),
    }
    let entries = fs::read_dir(root).map_err(|e| ArchonError::io("listing report archive", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| ArchonError::io("reading report archive entry", e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let owned = if ReportId::parse(name).is_ok() {
            true
        } else if let Some((report, nonce)) = name.strip_prefix('.').and_then(|s| s.split_once('-'))
        {
            ReportId::parse(report).is_ok() && ReportId::parse(nonce).is_ok()
        } else {
            false
        };
        if !owned || referenced.contains(&entry.path()) {
            continue;
        }
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| ArchonError::io("reading orphan snapshot metadata", e))?;
        if !meta.is_dir() {
            continue;
        }
        let modified: Timestamp = meta
            .modified()
            .map_err(|e| ArchonError::io("reading orphan snapshot age", e))?
            .into();
        if modified < cutoff {
            fs::remove_dir_all(entry.path())
                .map_err(|e| ArchonError::io("removing orphan report snapshot", e))?;
        }
    }
    Ok(())
}
