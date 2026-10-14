//! Who may use a data directory: archond holds an exclusive `flock` on
//! `<data>/data.lock` for its whole life, taken before it opens the store,
//! and anything else that opens the store there holds it shared. So two
//! daemons never share one store, and none starts while another process has
//! it open. The file is never removed, as with the socket's lock.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use archon_core::error::ArchonError;
use serde::{Deserialize, Serialize};

/// What archond writes into the lock file it holds, so a daemon it turns
/// away can say which one is in the way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub pid: u32,
    pub socket: PathBuf,
}

/// A held lock on a data directory; released when dropped.
#[derive(Debug)]
pub struct DataLock {
    _file: File,
    data_dir: PathBuf,
    exclusive: bool,
}

impl DataLock {
    /// archond's lock on `data_dir`, recording its pid and `socket`. Refused,
    /// naming who is in the way, while another archond or a process with
    /// the store open holds it.
    pub fn acquire(data_dir: &Path, socket: &Path) -> Result<Self, ArchonError> {
        assert!(
            data_dir.is_absolute(),
            "archond resolves its data dir first"
        );
        assert!(socket.is_absolute(), "archond resolves its socket first");
        let path = archon_core::paths::data_lock_path_for(data_dir);
        let file = open(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(turned_away(&path, data_dir)),
            Err(TryLockError::Error(e)) => {
                return Err(ArchonError::io(format!("locking {}", path.display()), e));
            }
        }
        let holder = Holder {
            pid: std::process::id(),
            socket: socket.to_owned(),
        };
        record(&file, &path, &holder)?;
        Ok(Self {
            _file: file,
            data_dir: data_dir.to_owned(),
            exclusive: true,
        })
    }

    /// A shared lock on `data_dir` for a user of the store other than
    /// archond: refused while an archond holds it, and keeps one from
    /// starting meanwhile.
    pub fn share(data_dir: &Path) -> Result<Self, ArchonError> {
        let path = archon_core::paths::data_lock_path_for(data_dir);
        let file = open(&path)?;
        match file.try_lock_shared() {
            Ok(()) => Ok(Self {
                _file: file,
                data_dir: data_dir.to_owned(),
                exclusive: false,
            }),
            Err(TryLockError::WouldBlock) => Err(ArchonError::Unavailable(format!(
                "{} owns the data directory {}; ask it through its socket instead of \
                 opening its store",
                holder_text(&path),
                data_dir.display()
            ))),
            Err(TryLockError::Error(e)) => Err(ArchonError::io(
                format!("locking {} shared", path.display()),
                e,
            )),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// archond's own lock, as opposed to a shared one.
    pub fn is_exclusive(&self) -> bool {
        self.exclusive
    }
}

fn open(path: &Path) -> Result<File, ArchonError> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| ArchonError::io(format!("opening {}", path.display()), e))
}

/// Replace the file's contents with `holder`; only the exclusive holder
/// writes, so no reader sees two records at once.
fn record(file: &File, path: &Path, holder: &Holder) -> Result<(), ArchonError> {
    let text = serde_json::to_string(holder).map_err(|e| {
        ArchonError::Store(format!("encoding the holder of {}: {e}", path.display()))
    })?;
    assert!(!text.contains('\n'), "one line of JSON");
    let writing = |e| ArchonError::io(format!("writing {}", path.display()), e);
    file.set_len(0).map_err(writing)?;
    let mut writer = file;
    writer.write_all(text.as_bytes()).map_err(writing)?;
    writer.write_all(b"\n").map_err(writing)
}

/// Why `acquire` could not take the lock: an archond holds it, or only
/// shared holders do, which a shared probe tells apart.
fn turned_away(path: &Path, data_dir: &Path) -> ArchonError {
    let probe = match open(path) {
        Ok(probe) => probe,
        Err(error) => return error,
    };
    match probe.try_lock_shared() {
        Ok(()) => ArchonError::Unavailable(format!(
            "another process has the store in {} open without archond (`archon gc \
             --offline`, a test or a tool; it holds {} shared); start archond once it is done",
            data_dir.display(),
            path.display()
        )),
        Err(TryLockError::WouldBlock) => ArchonError::Unavailable(format!(
            "another {} owns the data directory {} (it holds {}); two daemons on one store \
             would undo each other's work. Stop that one first, or give this one a data \
             directory of its own with ARCHON_DATA_DIR",
            holder_text(path),
            data_dir.display(),
            path.display()
        )),
        Err(TryLockError::Error(e)) => {
            ArchonError::io(format!("probing the lock {}", path.display()), e)
        }
    }
}

/// `archond (pid 7, socket /run/…/archond.sock)`, from the holder's record.
fn holder_text(path: &Path) -> String {
    let read = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str::<Holder>(text.trim()).map_err(|e| e.to_string()));
    match read {
        Ok(holder) => format!(
            "archond (pid {}, socket {})",
            holder.pid,
            holder.socket.display()
        ),
        // Written just after the lock is taken, so a new holder may not
        // have written it yet.
        Err(why) => format!("archond (pid unknown: its record is unreadable, {why})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        (dir, path)
    }

    #[test]
    fn a_second_archond_is_turned_away_by_name() {
        let (_tmp, data) = dir();
        let first = DataLock::acquire(&data, Path::new("/run/a/archond.sock")).unwrap();
        assert!(first.is_exclusive());
        let error = DataLock::acquire(&data, Path::new("/run/b/archond.sock")).unwrap_err();
        let text = error.to_string();
        assert!(matches!(error, ArchonError::Unavailable(_)), "{text}");
        let pid = std::process::id();
        assert!(
            text.contains(&format!("archond (pid {pid}, socket /run/a/archond.sock)")),
            "{text}"
        );
        assert!(text.contains(&data.display().to_string()), "{text}");
        drop(first);
        let second = DataLock::acquire(&data, Path::new("/run/b/archond.sock")).unwrap();
        let record = std::fs::read_to_string(data.join("data.lock")).unwrap();
        assert!(
            record.contains("/run/b/archond.sock"),
            "the new holder: {record}"
        );
        drop(second);
    }

    #[test]
    fn shared_users_and_archond_exclude_each_other() {
        let (_tmp, data) = dir();
        let socket = Path::new("/run/a/archond.sock");
        let reader = DataLock::share(&data).unwrap();
        let another = DataLock::share(&data).unwrap();
        assert!(!reader.is_exclusive() && !another.is_exclusive());
        let refused = DataLock::acquire(&data, socket).unwrap_err().to_string();
        assert!(refused.contains("open without archond"), "{refused}");
        drop((reader, another));

        let daemon = DataLock::acquire(&data, socket).unwrap();
        let refused = DataLock::share(&data).unwrap_err().to_string();
        assert!(refused.contains("socket /run/a/archond.sock"), "{refused}");
        assert!(refused.contains("ask it through its socket"), "{refused}");
        drop(daemon);
        assert!(DataLock::share(&data).is_ok(), "free once archond is gone");
    }

    #[test]
    fn an_unreadable_record_still_names_archond() {
        let (_tmp, data) = dir();
        let _daemon = DataLock::acquire(&data, Path::new("/run/a/archond.sock")).unwrap();
        std::fs::write(data.join("data.lock"), "half a rec").unwrap();
        let refused = DataLock::share(&data).unwrap_err().to_string();
        assert!(refused.contains("archond (pid unknown"), "{refused}");
    }
}
