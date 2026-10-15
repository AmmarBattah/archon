//! The reviewers' experiment directories (ARCHITECTURE §13.5): one per item
//! at `<data>/experiments/<item id>`, its own root beside the scratch-mode
//! workspaces at `<data>/scratch/<item id>`, so neither holds the other.
//! Created before an agent that can review starts, set aside when the item
//! settles and deleted off the run table; gc deletes what a crash left.

use std::io::ErrorKind;
use std::path::PathBuf;

use archon_core::error::ArchonError;
use archon_core::id::QueueItemId;

use crate::context::DaemonPaths;

pub(crate) fn item_dir(paths: &DaemonPaths, item: &QueueItemId) -> PathBuf {
    let dir = paths.experiments().join(item.as_str());
    assert!(dir.is_absolute(), "the data directory is absolute");
    dir
}

/// Create the item's directory unless it exists.
pub(crate) async fn prepare(paths: &DaemonPaths, item: &QueueItemId) -> Result<(), ArchonError> {
    let dir = item_dir(paths, item);
    assert!(
        !dir.starts_with(&paths.scratch),
        "no scratch-mode workspace holds it"
    );
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| ArchonError::io(format!("creating {}", dir.display()), e))
}

/// Rename a settled item's directory aside to delete, with the run table held
/// so a retry that starts meanwhile creates a fresh one. `None` when it has none.
pub(crate) fn set_aside(paths: &DaemonPaths, item: &QueueItemId) -> Option<PathBuf> {
    let dir = item_dir(paths, item);
    match crate::gc::set_aside(&dir) {
        Ok(aside) => {
            assert_eq!(aside.parent(), dir.parent(), "set aside in place");
            Some(aside)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            // The item is settled, so the next gc pass collects it.
            tracing::warn!(dir = %dir.display(), %error, "experiments not set aside");
            None
        }
    }
}

/// Delete a directory [`set_aside`] renamed. Its name marks it claimed, so a
/// deletion that fails is logged and left to the next gc pass.
pub(crate) async fn delete(aside: PathBuf) {
    assert!(aside.is_absolute(), "set aside under the data directory");
    let target = aside.clone();
    let removed = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&target)).await;
    let error = match removed {
        Ok(Ok(())) => return,
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => return,
        Ok(Err(e)) => e.to_string(),
        Err(e) => format!("the removal task failed: {e}"),
    };
    tracing::warn!(dir = %aside.display(), %error, "experiments not deleted; gc retries");
}
