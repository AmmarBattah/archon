//! The disk hold's check and its one `Blocker` per episode (ARCHITECTURE
//! §7.3): low free space, or a write that failed on a full disk.

use std::time::Instant;

use archon_core::error::ArchonError;
use archon_core::notify::NotificationKind;
use archon_core::protocol::gib;

use crate::context::DaemonContext;
use crate::disk::{self, DiskHold, DiskReading};
use crate::executor;
use crate::feed;

/// Remembers the disk hold last announced: `low`, `full` or empty.
const DISK_HOLD_KEY: &str = "dispatch_disk_hold";

/// Read the free space where archond writes, update the hold and announce a
/// change. A failed read changes nothing: it is logged and shown in status.
pub(crate) async fn check(ctx: &DaemonContext) -> Result<(), ArchonError> {
    let reading = match read(ctx).await {
        Ok(reading) => reading,
        Err(error) => {
            tracing::warn!(%error, "free disk space unknown; the disk hold stays as it was");
            return Ok(());
        }
    };
    let min_free = ctx.config.limits.min_free_bytes();
    let hold = ctx.disk.update(reading, min_free, Instant::now());
    assert!(
        hold.as_ref().is_none_or(|h| h.min_free_bytes == min_free),
        "the hold is judged against the configured minimum"
    );
    announce(ctx, hold.as_ref()).await
}

/// The fullest watched path, read off the async workers.
pub(crate) async fn read(ctx: &DaemonContext) -> Result<DiskReading, ArchonError> {
    let probe = ctx.disk.probe();
    let paths = ctx.watched_dirs();
    assert!(
        !paths.is_empty(),
        "the data dir and worktree root are watched"
    );
    tokio::task::spawn_blocking(move || disk::lowest(&probe, &paths))
        .await
        .map_err(|e| ArchonError::Unavailable(format!("the free space check stopped: {e}")))?
}

/// Markers of the hold last announced, kept under [`DISK_HOLD_KEY`].
const LOW: &str = "low";
const FULL: &str = "full";
/// A disk-full hold for a failure the operator was already told of: it
/// starts without a `Blocker` and ends without a line, unless space runs
/// low meanwhile.
const FULL_AGAIN: &str = "full_again";

/// Post a `Blocker` when a hold starts, again only if a new disk-full
/// failure joins a low-disk hold or space runs low in a quiet one; a
/// system line when it ends. A disk-full failure already announced is not
/// announced again. Recorded only once posted, so a failed post is retried.
async fn announce(ctx: &DaemonContext, hold: Option<&DiskHold>) -> Result<(), ArchonError> {
    let last = ctx
        .store
        .lock()
        .await
        .kv_get(DISK_HOLD_KEY)?
        .unwrap_or_default();
    let marker = marker(hold, &last);
    if last == marker {
        return Ok(());
    }
    let unheard = last.is_empty() || last == FULL_AGAIN;
    match hold {
        Some(_) if marker == FULL_AGAIN => {}
        Some(hold) if unheard || (marker == FULL && !hold.already_announced) => {
            let text = blocker_text(hold);
            executor::notify(ctx, NotificationKind::Blocker, &text, None, None).await?;
            if let Some(full) = &hold.disk_full {
                ctx.disk.announced_full(full.cause);
            }
        }
        Some(_) => {}
        None if last == FULL_AGAIN => {}
        None => {
            feed::system(ctx, "Disk space is back; dispatch resumes.").await?;
        }
    }
    ctx.store.lock().await.kv_set(DISK_HOLD_KEY, marker)
}

/// The marker of `hold`, given the `last` one announced. Only a hold that
/// starts on a disk-full failure already announced, with room above
/// `limits.min_free_gb`, stays quiet: low space is always news.
fn marker(hold: Option<&DiskHold>, last: &str) -> &'static str {
    let starts = last.is_empty() || last == FULL_AGAIN;
    let marker = match hold {
        Some(h) if h.disk_full.is_some() && h.already_announced && starts && !h.is_low() => {
            FULL_AGAIN
        }
        Some(h) if h.disk_full.is_some() => FULL,
        Some(_) => LOW,
        None => "",
    };
    debug_assert!(
        hold.is_some_and(|h| h.already_announced && !h.is_low()) || marker != FULL_AGAIN,
        "only an announced failure with room is quiet"
    );
    debug_assert_eq!(hold.is_none(), marker.is_empty());
    marker
}

/// What the operator reads: the free space, where, why and what to run.
fn blocker_text(hold: &DiskHold) -> String {
    let free = gib(hold.reading.free_bytes);
    let path = hold.reading.path.display();
    let resume = gib(hold.resume_bytes());
    let full = hold
        .disk_full
        .as_ref()
        .map(|full| format!("a write failed on a full disk ({})", full.error));
    let low = hold.is_low().then(|| {
        format!(
            "free space is below limits.min_free_gb ({})",
            gib(hold.min_free_bytes)
        )
    });
    let cause = match (full, low) {
        (Some(full), Some(low)) => format!("{full}, and {low}"),
        (Some(cause), None) | (None, Some(cause)) => cause,
        (None, None) => unreachable!("a hold is low, full or both"),
    };
    format!(
        "Dispatch is held: {cause}. {free} free at {path}. No new agent starts, and \
         retries wait, until {resume} are free; running agents finish. \
         Run `archon gc` to reclaim space."
    )
}
