//! HTML pages stages write (ARCHITECTURE §14): the daemon publishes the
//! files a completed stage lists in `structured.reports[]`.

use std::path::{Component, Path, PathBuf};

use archon_core::error::ArchonError;
use archon_core::id::StageRunId;
use archon_core::run::{GateSubject, PipelineRun, StageRun, StageState};
use archon_pipeline::OperatorWait;

use super::FollowUp;
use crate::context::DaemonContext;
use crate::{feed, reports};

/// Where a gate stage's prompt tells it to write its page.
pub(super) fn conventional_page(subject: GateSubject) -> &'static str {
    match subject {
        GateSubject::Proposal => ".archon/reports/proposal/index.html",
        GateSubject::Report => ".archon/reports/change/index.html",
    }
}

/// Each page `stage` lists, or why it is refused: a stage publishes only
/// files inside its run's workspace. Relative paths are taken from there.
pub(super) fn listed_pages(run: &PipelineRun, stage: &StageRun) -> Vec<Result<PathBuf, String>> {
    assert_eq!(stage.run, run.id, "the stage belongs to the run");
    assert!(run.workspace.is_absolute(), "run workspaces are absolute");
    let Some(listed) = stage.structured.as_ref().and_then(|s| s.get("reports")) else {
        return Vec::new();
    };
    let Some(listed) = listed.as_array() else {
        return vec![Err(format!(
            "`reports` must be a list of paths, not {listed}"
        ))];
    };
    listed
        .iter()
        .map(|entry| match entry.as_str() {
            Some(raw) => inside_workspace(&run.workspace, raw),
            None => Err(format!("report entry {entry} is not a path")),
        })
        .collect()
}

pub(super) fn inside_workspace(workspace: &Path, raw: &str) -> Result<PathBuf, String> {
    let path = workspace.join(raw.trim());
    let climbs = path.components().any(|c| c == Component::ParentDir);
    if raw.trim().is_empty() || climbs || !path.starts_with(workspace) {
        return Err(format!(
            "{raw:?} is not a path inside the run's workspace {}",
            workspace.display()
        ));
    }
    // Keep the caller's path for snapshot validation, but never publish a
    // symlink that resolves outside the run's workspace.
    if let (Ok(resolved), Ok(root)) = (path.canonicalize(), workspace.canonicalize())
        && !resolved.starts_with(root)
    {
        return Err(format!(
            "report {} resolves outside the run's workspace",
            path.display()
        ));
    }
    Ok(path)
}

/// The page a gate shows: the first workspace path its stage listed, else
/// the conventional page if written. Announcement reports listing errors
/// from the stored stages and validates the selected file before publishing.
pub(super) fn gate_page(
    run: &PipelineRun,
    stages: &[StageRun],
    wait: &OperatorWait,
) -> Option<PathBuf> {
    if let Some(page) = gate_pages(run, stages, &wait.stage_id, wait.attempt)
        .into_iter()
        .find_map(Result::ok)
    {
        return Some(page);
    }
    let page = run.workspace.join(conventional_page(wait.subject));
    page.is_file().then_some(page)
}

/// Listing results remain in the stage records so gate announcement can
/// expose every refusal, including after a daemon restart.
pub(super) fn gate_pages(
    run: &PipelineRun,
    stages: &[StageRun],
    stage_id: &str,
    attempt: u32,
) -> Vec<Result<PathBuf, String>> {
    stages
        .iter()
        .filter(|s| s.stage_id == stage_id && s.attempt == attempt)
        .filter(|s| s.state == StageState::Completed)
        .flat_map(|s| listed_pages(run, s))
        .collect()
}

/// Whether a settled stage has pages to publish.
pub(super) fn lists_pages(stage: &StageRun) -> bool {
    let listed = stage.structured.as_ref().and_then(|s| s.get("reports"));
    stage.state == StageState::Completed
        && listed.is_some_and(|r| r.as_array().is_none_or(|a| !a.is_empty()))
}

/// Publish every page a completed stage listed. A page that cannot be
/// published is named in a `System` note; store failures propagate.
pub(super) async fn publish_stage(
    ctx: &DaemonContext,
    id: &StageRunId,
) -> Result<FollowUp, ArchonError> {
    let (stage, run, item) = {
        let store = ctx.store.lock().await;
        let stage = store.stage_run(id)?;
        let run = store.run(&stage.run)?;
        let item = store.item(&run.item)?;
        (stage, run, item)
    };
    assert_eq!(
        stage.state,
        StageState::Completed,
        "only completed stages publish"
    );
    let title = format!("{} ({} stage)", item.title, stage.stage_id);
    for page in listed_pages(&run, &stage) {
        let refused = match page {
            Ok(path) => match reports::publish(ctx, &title, &path, Some(&item.id)).await {
                Ok(_) => continue,
                Err(ArchonError::Invalid(why) | ArchonError::NotFound(why)) => why,
                Err(error) => return Err(error),
            },
            Err(why) => why,
        };
        let text = format!(
            "Stage `{}` of {} listed a report Archon did not publish: {refused}",
            stage.stage_id, item.id
        );
        feed::system(ctx, text).await?;
    }
    Ok(FollowUp::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_pages_stay_inside_the_workspace() {
        let workspace = Path::new("/w/run");
        let inside = inside_workspace(workspace, "/w/run/.archon/reports/a/index.html");
        assert_eq!(
            inside.unwrap(),
            workspace.join(".archon/reports/a/index.html")
        );
        let relative = inside_workspace(workspace, ".archon/reports/b/index.html");
        assert_eq!(
            relative.unwrap(),
            workspace.join(".archon/reports/b/index.html")
        );
        assert!(inside_workspace(workspace, "/etc/passwd.html").is_err());
        assert!(inside_workspace(workspace, "/w/run/../other/index.html").is_err());
        assert!(inside_workspace(workspace, "../x.html").is_err());
        assert!(inside_workspace(workspace, " ").is_err());
    }
}
