//! Grading (ARCHITECTURE §15). Graders never run in the agent's workspace:
//! once the arm has stopped, the trial result is copied (or, for the archon
//! arm, checked out from its run branches) into the grading copy, and
//! `grade.sh` brings its hidden tests there. The copy is deleted afterwards;
//! each repository's diff against `bench-base` stays in the logs.
//!
//! `grade.sh` contract: working directory is the result repository (single
//! repo) or the directory holding every result repository (multi repo);
//! `BENCH_RESULT_<NAME>` points at each repository, `BENCH_TASK_DIR` at the
//! task (for `hidden/`), `BENCH_ANSWER` at the operator-visible final text,
//! and `BENCH_GRADERS` at the shared helpers. Exit 0 means pass; an optional
//! last stdout line `{"score": 0..1}` grades partial credit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, ensure};

use crate::process::{self, CommandSpec};
use crate::task::BenchTask;
use crate::workspace::{self, BASE_TAG, TrialLayout};

/// Generous: Rust graders compile their fixture from scratch.
pub const GRADE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Where an arm left its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultSource {
    /// The agent's working tree, uncommitted changes included.
    Workspace,
    /// Archon: per fixture name, the run branches of its succeeded items in
    /// completion order. They are merged onto `bench-base` for grading.
    Branches(BTreeMap<String, Vec<String>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradeOutcome {
    pub success: bool,
    pub score: Option<f64>,
    /// Result problems that are the arm's fault, e.g. conflicting branches.
    pub note: Option<String>,
}

/// Shared grader helpers (`rubric.py`).
pub fn graders_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("graders")
}

/// Build `grade/` from the result, then run `grade.sh` there. `label` becomes
/// `BENCH_ARM`: the arm, or `untouched`/`reference` when verifying.
pub async fn grade(
    task: &BenchTask,
    layout: &TrialLayout,
    label: &str,
    source: &ResultSource,
) -> anyhow::Result<GradeOutcome> {
    let mut notes: Vec<String> = assemble(task, layout, source)?.into_iter().collect();
    save_result_patches(task, layout)?;
    let spec = grader_command(task, layout, label);
    let stdout = layout.logs.join("grade.stdout");
    let stderr = layout.logs.join("grade.stderr");
    let run = process::run(&spec, &stdout, &stderr, GRADE_TIMEOUT).await?;
    let text = std::fs::read_to_string(&stdout)
        .with_context(|| format!("reading {}", stdout.display()))?;
    if run.timed_out {
        notes.push("the grader timed out".to_owned());
    }
    if let Err(e) = std::fs::remove_dir_all(&layout.grade) {
        notes.push(format!(
            "the grading copy {} was not removed: {e}",
            layout.grade.display()
        ));
    }
    Ok(GradeOutcome {
        success: run.success(),
        score: parse_score(&text),
        note: (!notes.is_empty()).then(|| notes.join("; ")),
    })
}

/// `logs/<repo>.result.patch`: everything the result changed against
/// `bench-base`, untracked files included, for review after the copy is gone.
fn save_result_patches(task: &BenchTask, layout: &TrialLayout) -> anyhow::Result<()> {
    assert!(layout.grade.is_dir(), "the grading copy is assembled first");
    for fixture in task.fixtures() {
        let repo = layout.grade.join(&fixture.name);
        ensure!(repo.is_dir(), "{} is missing", repo.display());
        workspace::git(&repo, &["add", "-A"])?;
        let patch = workspace::git(&repo, &["diff", "--cached", "--binary", BASE_TAG])?;
        let path = layout.logs.join(format!("{}.result.patch", fixture.name));
        std::fs::write(&path, patch).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// Copy or check out every result repository into `layout.grade`.
pub fn assemble(
    task: &BenchTask,
    layout: &TrialLayout,
    source: &ResultSource,
) -> anyhow::Result<Option<String>> {
    ensure!(
        !layout.grade.exists(),
        "{} already exists; grading runs once per trial",
        layout.grade.display()
    );
    std::fs::create_dir_all(&layout.grade)?;
    let mut notes = Vec::new();
    for fixture in task.fixtures() {
        let (repo, target) = (layout.repo(&fixture), layout.grade.join(&fixture.name));
        match source {
            ResultSource::Workspace => workspace::copy_tree(&repo, &target)?,
            ResultSource::Branches(branches) => {
                let wanted = branches
                    .get(&fixture.name)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if let Some(note) = checkout_branches(&repo, &target, wanted)? {
                    notes.push(format!("{}: {note}", fixture.name));
                }
            }
        }
    }
    Ok((!notes.is_empty()).then(|| notes.join("; ")))
}

/// A detached worktree at `bench-base` with each branch merged in order.
/// A conflicting branch is left out and reported: the operator could not
/// combine that work either.
fn checkout_branches(
    repo: &Path,
    target: &Path,
    branches: &[String],
) -> anyhow::Result<Option<String>> {
    let target_text = target.to_string_lossy();
    workspace::git(
        repo,
        &["worktree", "add", "-q", "--detach", &target_text, BASE_TAG],
    )?;
    let mut skipped = Vec::new();
    for branch in branches {
        let merged = workspace::git(target, &["merge", "-q", "--no-ff", "--no-edit", branch]);
        if let Err(error) = merged {
            workspace::git(target, &["merge", "--abort"])
                .with_context(|| format!("aborting the failed merge of {branch}: {error:#}"))?;
            skipped.push(branch.clone());
        }
    }
    if skipped.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "branches {skipped:?} conflicted and were left out"
    )))
}

/// The grader invocation; pure so tests and `--dry-run` can inspect it.
pub fn grader_command(task: &BenchTask, layout: &TrialLayout, label: &str) -> CommandSpec {
    let fixtures = task.fixtures();
    let cwd = if task.is_multi_repo() {
        layout.grade.clone()
    } else {
        layout.grade.join(&fixtures[0].name)
    };
    let mut spec = CommandSpec::new("sh", cwd);
    spec.args = vec![task.grade_script().to_string_lossy().into_owned()];
    for fixture in &fixtures {
        let path = layout.grade.join(&fixture.name);
        spec.env
            .push((fixture.env_var(), path.to_string_lossy().into_owned()));
    }
    let ceiling = layout.grade.to_string_lossy().into_owned();
    spec.env.extend([
        (
            "BENCH_TASK_DIR".to_owned(),
            task.dir.to_string_lossy().into_owned(),
        ),
        (
            "BENCH_ANSWER".to_owned(),
            layout.answer_file().to_string_lossy().into_owned(),
        ),
        (
            "BENCH_GRADERS".to_owned(),
            graders_dir().to_string_lossy().into_owned(),
        ),
        ("BENCH_ARM".to_owned(), label.to_owned()),
        ("GIT_CEILING_DIRECTORIES".to_owned(), ceiling),
        ("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned()),
    ]);
    spec.env_remove = vec!["CARGO_TARGET_DIR".into(), "CARGO_BUILD_TARGET_DIR".into()];
    spec
}

/// The last stdout line, when it is `{"score": x}` with x in 0..=1.
pub fn parse_score(stdout: &str) -> Option<f64> {
    let line = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    value["score"].as_f64().filter(|s| (0.0..=1.0).contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::scratch;

    fn bundled(id: &str) -> BenchTask {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks");
        BenchTask::load(&root.join(id)).unwrap()
    }

    #[test]
    fn scores_come_from_the_last_json_line_only() {
        assert_eq!(parse_score("ok\n{\"score\": 0.75}\n\n"), Some(0.75));
        assert_eq!(parse_score("{\"score\": 0.5}\ntrailing text"), None);
        assert_eq!(parse_score("{\"score\": 1.5}"), None);
        assert_eq!(parse_score(""), None);
    }

    #[test]
    fn grader_command_exports_every_result_path() {
        let task = bundled("fix-off-by-one");
        let layout = TrialLayout::new("/w/ws".into(), "/w/g".into(), "/r/logs".into());
        let spec = grader_command(&task, &layout, "codex");
        assert_eq!(spec.cwd, Path::new("/w/g/fix-off-by-one"));
        let env: BTreeMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(env["BENCH_RESULT_FIX_OFF_BY_ONE"], "/w/g/fix-off-by-one");
        assert_eq!(env["BENCH_ANSWER"], "/r/logs/answer.txt");
        assert_eq!(env["GIT_CEILING_DIRECTORIES"], "/w/g");
        assert_eq!(env["BENCH_ARM"], "codex");
        assert!(spec.args[0].ends_with("fix-off-by-one/grade.sh"));
    }

    #[tokio::test]
    async fn grading_uses_a_throwaway_copy_and_keeps_the_patch() {
        let tmp = scratch();
        let task = bundled("fix-off-by-one");
        let layout = TrialLayout::under(&tmp.path().join("t"));
        workspace::prepare(&task, &layout).unwrap();
        let repo = layout.agent_cwd(&task);
        std::fs::write(repo.join("notes.txt"), "agent work\n").unwrap();
        std::fs::write(layout.answer_file(), "").unwrap();
        let graded = grade(&task, &layout, "claude", &ResultSource::Workspace)
            .await
            .unwrap();
        assert!(!graded.success, "the untouched bug still fails");
        assert!(
            !layout.grade.exists(),
            "hidden tests are gone with the copy"
        );
        let hidden = std::fs::read_dir(task.dir.join("hidden")).unwrap();
        for entry in hidden {
            let name = entry.unwrap().file_name();
            assert!(
                !repo.join(&name).exists(),
                "{name:?} leaked into the workspace"
            );
        }
        let patch = layout.logs.join("fix-off-by-one.result.patch");
        let patch = std::fs::read_to_string(patch).unwrap();
        assert!(patch.contains("+agent work"), "{patch}");
    }

    #[tokio::test]
    async fn a_second_grading_keeps_its_own_logs_beside_the_verdicts() {
        let tmp = scratch();
        let task = bundled("fix-off-by-one");
        let layout = TrialLayout::under(&tmp.path().join("t"));
        workspace::prepare(&task, &layout).unwrap();
        let repo = layout.agent_cwd(&task);
        std::fs::write(layout.answer_file(), "").unwrap();
        let verdict = grade(&task, &layout, "archon", &ResultSource::Workspace)
            .await
            .unwrap();
        std::fs::write(repo.join("late.txt"), "committed late\n").unwrap();
        std::fs::write(layout.answer_file(), "the final answer\n").unwrap();
        let late_layout = layout.late_grade().unwrap();
        let answer = std::fs::read_to_string(late_layout.answer_file()).unwrap();
        assert_eq!(answer, "the final answer\n", "graders read the same answer");
        let late = grade(&task, &late_layout, "archon", &ResultSource::Workspace)
            .await
            .unwrap();
        assert!(!verdict.success && !late.success);
        let (logs, late_logs) = (&layout.logs, &late_layout.logs);
        assert_eq!(late_logs, &logs.join(workspace::TIMEOUT_LOG_DIR));
        for dir in [logs, late_logs] {
            assert!(
                dir.join("grade.stdout").is_file(),
                "{} lacks grade.stdout",
                dir.display()
            );
        }
        let first = std::fs::read_to_string(logs.join("fix-off-by-one.result.patch")).unwrap();
        let second =
            std::fs::read_to_string(late_logs.join("fix-off-by-one.result.patch")).unwrap();
        assert!(!first.contains("late.txt") && second.contains("+committed late"));
    }

    #[test]
    fn archon_branches_are_merged_onto_the_base() {
        let tmp = scratch();
        let task = bundled("fix-off-by-one");
        let layout = TrialLayout::under(&tmp.path().join("t"));
        workspace::prepare(&task, &layout).unwrap();
        let repo = layout.agent_cwd(&task);
        for (branch, file) in [("archon/a", "a.txt"), ("archon/b", "b.txt")] {
            workspace::git(&repo, &["checkout", "-q", "-b", branch, BASE_TAG]).unwrap();
            std::fs::write(repo.join(file), branch).unwrap();
            std::fs::write(repo.join("window.py"), branch).unwrap();
            workspace::git(&repo, &["add", "-A"]).unwrap();
            workspace::git(&repo, &["commit", "-q", "-m", branch]).unwrap();
        }
        // Uncommitted work never reaches the archon result.
        std::fs::write(repo.join("stray.txt"), "x").unwrap();
        let branches = BTreeMap::from([(
            "fix-off-by-one".to_owned(),
            vec!["archon/a".to_owned(), "archon/b".to_owned()],
        )]);
        let note = assemble(&task, &layout, &ResultSource::Branches(branches)).unwrap();
        let graded = layout.grade.join("fix-off-by-one");
        assert!(graded.join("a.txt").is_file());
        assert!(
            !graded.join("b.txt").exists(),
            "the conflicting branch is left out"
        );
        assert!(!graded.join("stray.txt").exists());
        assert!(note.unwrap().contains("archon/b"));
    }
}
