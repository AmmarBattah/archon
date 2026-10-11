//! Free disk space where archond writes, and the dispatch hold it drives
//! (ARCHITECTURE §7.3): a low-disk hold while the data dir or the worktree
//! root has less than `limits.min_free_gb` free, and a disk-full hold after
//! a stage, a spawn or the store failed on a full disk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use archon_core::error::ArchonError;

/// Reads the bytes an unprivileged process may still write under a path.
pub type FreeSpaceProbe = Arc<dyn Fn(&Path) -> std::io::Result<u64> + Send + Sync>;

/// A disk-full failure holds dispatch at least this long, so a disk that
/// fills again at once is not retried in a tight loop.
pub const DISK_FULL_HOLD: Duration = Duration::from_secs(5 * 60);

/// After a disk-full failure, dispatch resumes only with this much free
/// (or `limits.min_free_gb`, if more), even with the low-disk hold off.
pub const DISK_FULL_RESUME_BYTES: u64 = 1 << 30;

/// What SQLite says on a full disk: `SQLITE_FULL`'s text, and rusqlite's
/// `DiskFull` code name.
const DATABASE_FULL_MARKERS: [&str; 2] = ["database or disk is full", "diskfull"];
/// What any other write says on a full disk: `ENOSPC`.
const FILE_FULL_MARKERS: [&str; 2] = ["no space left on device", "enospc"];

/// Whether an error message says the disk it wrote to is full.
pub fn is_disk_full(error: &str) -> bool {
    let error = error.to_lowercase();
    DATABASE_FULL_MARKERS
        .iter()
        .chain(&FILE_FULL_MARKERS)
        .any(|marker| error.contains(marker))
}

/// Whose disk a disk-full failure was on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullDisk {
    /// Archon's own: its store failed to write.
    Archon,
    /// Aether's or an agent's: a stage or a `create_agent` failed on it.
    Aether,
}

/// Which write failed on a full disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullWrite {
    /// SQLite's (`SQLITE_FULL`).
    Database,
    /// Any other (`ENOSPC`).
    File,
}

/// A disk-full failure's cause, whatever its exact text: two failures with
/// one cause are the same news, however the messages differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullCause {
    pub disk: FullDisk,
    pub write: FullWrite,
}

impl FullCause {
    /// The cause of `error`, a disk-full failure on `disk`: the store's
    /// [`ArchonError::DiskFull`] on Archon's, else a text [`is_disk_full`]
    /// recognises; anything else is a caller's bug. A failure that names no
    /// `ENOSPC` is SQLite's, since the store's `DiskFull` is `SQLITE_FULL`.
    pub fn of(disk: FullDisk, error: &str) -> Self {
        debug_assert!(
            is_classified(disk, error),
            "not a disk-full failure on {disk:?}'s disk: {error}"
        );
        let error = error.to_lowercase();
        let file = FILE_FULL_MARKERS.iter().any(|m| error.contains(m));
        let write = if file {
            FullWrite::File
        } else {
            FullWrite::Database
        };
        Self { disk, write }
    }
}

/// Whether `error` is a disk-full failure its caller classified for `disk`:
/// the store's own error on Archon's disk, a recognised text on Aether's.
fn is_classified(disk: FullDisk, error: &str) -> bool {
    match disk {
        FullDisk::Archon => error.starts_with(&ArchonError::DiskFull(String::new()).to_string()),
        FullDisk::Aether => is_disk_full(error),
    }
}

/// The disk-full failure behind a hold: its latest text and its cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFull {
    pub error: String,
    pub cause: FullCause,
}

/// `statvfs` of `path`, or of its nearest existing ancestor (a worktree
/// root not created yet lives on its parent's filesystem).
pub fn free_bytes(path: &Path) -> std::io::Result<u64> {
    let existing = path
        .ancestors()
        .find(|p| p.exists())
        .unwrap_or(Path::new("."));
    let stat = rustix::fs::statvfs(existing)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

/// The free space of one watched path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskReading {
    pub path: PathBuf,
    pub free_bytes: u64,
}

/// The watched path with the least free space. Blocking: call it off the
/// async workers.
pub fn lowest(probe: &FreeSpaceProbe, paths: &[PathBuf]) -> Result<DiskReading, ArchonError> {
    assert!(!paths.is_empty(), "at least one path is watched");
    let mut lowest: Option<DiskReading> = None;
    for path in paths {
        let free_bytes = probe(path).map_err(|e| {
            ArchonError::io(format!("reading the free space at {}", path.display()), e)
        })?;
        if lowest.as_ref().is_none_or(|l| free_bytes < l.free_bytes) {
            let path = path.clone();
            lowest = Some(DiskReading { path, free_bytes });
        }
    }
    let lowest = lowest.expect("one reading per path");
    debug_assert!(paths.contains(&lowest.path));
    Ok(lowest)
}

/// Why dispatch is held for disk space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskHold {
    pub reading: DiskReading,
    /// `limits.min_free_gb` in bytes; 0 when the low-disk hold is off.
    pub min_free_bytes: u64,
    /// The disk-full failure behind the hold; `None` for low space alone.
    pub disk_full: Option<DiskFull>,
    /// The operator was already told of a disk-full failure with this
    /// cause in an earlier episode, and no agent was created since.
    pub already_announced: bool,
}

impl DiskHold {
    /// Whether free space is below `limits.min_free_gb`.
    pub fn is_low(&self) -> bool {
        self.min_free_bytes > 0 && self.reading.free_bytes < self.min_free_bytes
    }

    /// The free space dispatch waits for.
    pub fn resume_bytes(&self) -> u64 {
        match self.disk_full {
            Some(_) => self.min_free_bytes.max(DISK_FULL_RESUME_BYTES),
            None => self.min_free_bytes,
        }
    }
}

/// The disk hold shared by the dispatcher, settles and spawns. The probe
/// and the disk-full hold are injected so tests can fill and free a disk.
pub struct DiskWatch {
    probe: FreeSpaceProbe,
    /// How long a disk-full failure holds at least; [`DISK_FULL_HOLD`].
    full_hold: Duration,
    state: Mutex<WatchState>,
}

#[derive(Debug, Default)]
struct WatchState {
    /// The last disk-full failure and when it was seen.
    full: Option<(Instant, DiskFull)>,
    /// Whether the last check held dispatch.
    held: bool,
    /// The cause of the disk-full failure the operator was last told of,
    /// until an agent is created: a hold that repeats it is not announced.
    announced: Option<FullCause>,
}

impl std::fmt::Debug for DiskWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiskWatch")
            .field("state", &*self.lock())
            .finish_non_exhaustive()
    }
}

impl DiskWatch {
    pub fn new(probe: FreeSpaceProbe, full_hold: Duration) -> Self {
        Self {
            probe,
            full_hold,
            state: Mutex::new(WatchState::default()),
        }
    }

    /// The real filesystem, through `statvfs`.
    pub fn system() -> Self {
        Self::new(Arc::new(free_bytes), DISK_FULL_HOLD)
    }

    pub fn probe(&self) -> FreeSpaceProbe {
        self.probe.clone()
    }

    fn lock(&self) -> MutexGuard<'_, WatchState> {
        self.state.lock().expect("no disk watch holder panics")
    }

    /// Record a failure on the full `disk`: dispatch holds from now on, for
    /// at least the disk-full hold and until enough space is free again.
    pub fn report_full(&self, error: &str, disk: FullDisk, now: Instant) {
        assert!(
            !error.trim().is_empty(),
            "a disk-full failure says what failed"
        );
        let full = DiskFull {
            error: error.to_owned(),
            cause: FullCause::of(disk, error),
        };
        let mut state = self.lock();
        state.full = Some((now, full));
        state.held = true;
    }

    /// Whether dispatch is held now: by the last check, or by a disk-full
    /// failure reported since.
    pub fn holding(&self) -> bool {
        let state = self.lock();
        state.held || state.full.is_some()
    }

    /// Update the hold from a fresh `reading`. A disk-full hold ends once
    /// its time has passed and [`DiskHold::resume_bytes`] are free; the
    /// low-disk hold ends as soon as `min_free_bytes` are.
    pub fn update(
        &self,
        reading: DiskReading,
        min_free_bytes: u64,
        now: Instant,
    ) -> Option<DiskHold> {
        let resume = min_free_bytes.max(DISK_FULL_RESUME_BYTES);
        let mut state = self.lock();
        if let Some((at, _)) = &state.full
            && now.saturating_duration_since(*at) >= self.full_hold
            && reading.free_bytes >= resume
        {
            state.full = None;
        }
        let disk_full = state.full.as_ref().map(|(_, full)| full.clone());
        let cause = disk_full.as_ref().map(|full| full.cause);
        let hold = DiskHold {
            reading,
            min_free_bytes,
            already_announced: cause.is_some() && cause == state.announced,
            disk_full,
        };
        let hold = (hold.is_low() || hold.disk_full.is_some()).then_some(hold);
        state.held = hold.is_some();
        debug_assert!(
            hold.as_ref()
                .is_none_or(|h| h.disk_full.is_some() || h.is_low()),
            "a low-disk hold only below the minimum"
        );
        hold
    }

    /// The disk-full failure holding dispatch, if any.
    pub fn disk_full(&self) -> Option<String> {
        self.lock()
            .full
            .as_ref()
            .map(|(_, full)| full.error.clone())
    }

    /// The operator was told of a disk-full failure with `cause`; later
    /// holds for the same cause are not announced again.
    pub fn announced_full(&self, cause: FullCause) {
        self.lock().announced = Some(cause);
    }

    /// An agent was created, so the writes behind the last announced
    /// disk-full failure succeed again: its next hold is news.
    pub fn agent_created(&self) {
        self.lock().announced = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn reading(free_bytes: u64) -> DiskReading {
        DiskReading {
            path: PathBuf::from("/data"),
            free_bytes,
        }
    }

    #[test]
    fn disk_full_errors_are_recognised_whatever_wrote_them() {
        for error in [
            "internal error: sqlite: database or disk is full",
            "write target/debug/x: No space left on device (os error 28)",
            "ENOSPC while writing the log",
            "SqliteFailure(Error { code: DiskFull, extended_code: 13 })",
        ] {
            assert!(is_disk_full(error), "{error}");
        }
        for error in ["disk quota check passed", "the agent failed", ""] {
            assert!(!is_disk_full(error), "{error}");
        }
    }

    #[test]
    fn the_fullest_watched_path_is_the_reading() {
        let probe: FreeSpaceProbe = Arc::new(|path: &Path| match path.to_str() {
            Some("/data") => Ok(50 * GIB),
            Some("/trees") => Ok(3 * GIB),
            _ => Err(std::io::Error::other("no such filesystem")),
        });
        let paths = [PathBuf::from("/data"), PathBuf::from("/trees")];
        let low = lowest(&probe, &paths).unwrap();
        assert_eq!(low.path, PathBuf::from("/trees"));
        assert_eq!(low.free_bytes, 3 * GIB);
        let error = lowest(&probe, &[PathBuf::from("/gone")]).unwrap_err();
        assert!(error.to_string().contains("/gone"), "{error}");
    }

    #[test]
    fn the_low_disk_hold_follows_free_space_and_zero_turns_it_off() {
        let watch = DiskWatch::new(Arc::new(|_: &Path| Ok(0)), DISK_FULL_HOLD);
        let now = Instant::now();
        let held = watch.update(reading(9 * GIB), 10 * GIB, now).unwrap();
        assert_eq!(held.disk_full, None);
        assert_eq!(held.resume_bytes(), 10 * GIB);
        assert!(watch.holding());
        assert_eq!(watch.update(reading(10 * GIB), 10 * GIB, now), None);
        assert!(!watch.holding());
        assert_eq!(watch.update(reading(0), 0, now), None, "0 disables it");
    }

    #[test]
    fn a_disk_full_failure_holds_for_a_while_and_until_space_is_back() {
        let watch = DiskWatch::new(Arc::new(|_: &Path| Ok(0)), DISK_FULL_HOLD);
        let start = Instant::now();
        let full = "store: disk full: insert: database or disk is full";
        watch.report_full(full, FullDisk::Archon, start);
        assert!(watch.holding(), "held at once, before any check");
        let plenty = reading(100 * GIB);
        let early = watch.update(plenty.clone(), 0, start + Duration::from_secs(60));
        assert!(early.is_some_and(|h| h.disk_full.is_some()), "too soon");
        let later = start + DISK_FULL_HOLD;
        let tight = watch
            .update(reading(DISK_FULL_RESUME_BYTES - 1), 0, later)
            .unwrap();
        assert_eq!(tight.resume_bytes(), DISK_FULL_RESUME_BYTES);
        assert_eq!(watch.update(plenty, 0, later), None);
        assert!(!watch.holding());
        assert_eq!(watch.disk_full(), None);
    }

    /// Whether a check now finds the disk-full hold's failure announced.
    fn announced(watch: &DiskWatch) -> bool {
        let hold = watch.update(reading(0), 0, Instant::now()).expect("held");
        hold.already_announced
    }

    /// Tell the operator of the failure holding dispatch now.
    fn announce(watch: &DiskWatch) {
        let hold = watch.update(reading(0), 0, Instant::now()).expect("held");
        watch.announced_full(hold.disk_full.expect("a disk-full hold").cause);
    }

    #[test]
    fn a_disk_full_failure_already_announced_is_known_until_an_agent_is_created() {
        let watch = DiskWatch::new(Arc::new(|_: &Path| Ok(0)), Duration::ZERO);
        let now = Instant::now();
        let full = "aether: database or disk is full";
        watch.report_full(full, FullDisk::Aether, now);
        assert!(!announced(&watch), "not told yet");
        announce(&watch);
        assert!(announced(&watch));
        assert_eq!(watch.update(reading(100 * GIB), 0, now), None);
        watch.report_full(full, FullDisk::Aether, now);
        assert!(announced(&watch), "the same failure in a new episode");
        watch.report_full("no space left on device", FullDisk::Aether, now);
        assert!(!announced(&watch), "another failure is news");
        watch.report_full(full, FullDisk::Aether, now);
        watch.agent_created();
        assert!(!announced(&watch), "an agent was created since");
    }

    #[test]
    fn a_disk_full_failure_is_the_same_news_whatever_its_exact_text() {
        let watch = DiskWatch::new(Arc::new(|_: &Path| Ok(0)), Duration::ZERO);
        let now = Instant::now();
        watch.report_full(
            "aether: internal: database or disk is full (request 1)",
            FullDisk::Aether,
            now,
        );
        announce(&watch);
        let again = "aether: internal: SqliteFailure { code: DiskFull } (request 2)";
        watch.report_full(again, FullDisk::Aether, now);
        assert!(announced(&watch), "the same cause, reworded");
        assert_eq!(watch.disk_full().as_deref(), Some(again), "latest text");
        watch.report_full("store: disk full: insert", FullDisk::Archon, now);
        assert!(!announced(&watch), "Archon's own disk is other news");
        let enospc = "engine: No space left on device (os error 28)";
        watch.report_full(enospc, FullDisk::Aether, now);
        assert!(!announced(&watch), "another write is other news");
    }

    #[test]
    fn a_disk_full_cause_names_the_disk_and_the_write() {
        let sqlite = FullCause::of(FullDisk::Aether, "Database or disk is full");
        assert_eq!(sqlite.write, FullWrite::Database);
        assert_eq!(sqlite.disk, FullDisk::Aether);
        let file = FullCause::of(FullDisk::Aether, "write x: ENOSPC");
        assert_eq!(file.write, FullWrite::File);
        assert_eq!(file.disk, FullDisk::Aether);
        let store = FullCause::of(FullDisk::Archon, "store: disk full: insert");
        assert_eq!(store.write, FullWrite::Database, "the store's DiskFull");
        assert_eq!(store.disk, FullDisk::Archon);
    }

    #[test]
    fn only_a_classified_disk_full_failure_has_a_cause() {
        let store = ArchonError::DiskFull("insert: database or disk is full".into());
        assert!(is_classified(FullDisk::Archon, &store.to_string()));
        assert!(is_classified(FullDisk::Aether, "engine: ENOSPC"));
        let unrecognised = "aether: internal: the agent failed";
        assert!(!is_classified(FullDisk::Aether, unrecognised));
        let not_the_store = ArchonError::Store("insert: locked".into());
        assert!(!is_classified(FullDisk::Archon, &not_the_store.to_string()));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not a disk-full failure")]
    fn an_unclassified_failure_is_a_caller_s_bug() {
        FullCause::of(FullDisk::Aether, "aether: internal: the agent failed");
    }

    #[test]
    fn the_system_probe_reads_a_real_directory_and_a_missing_one_s_parent() {
        let dir = tempfile::tempdir().unwrap();
        let free = free_bytes(dir.path()).unwrap();
        assert!(free > 0, "the test's own temp directory has room");
        let missing = dir.path().join("not/yet/made");
        assert!(free_bytes(&missing).unwrap() > 0, "read on the parent");
    }
}
