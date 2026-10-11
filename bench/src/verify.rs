//! Grader self-check: every task's `grade.sh` must fail on the untouched
//! fixture and pass once its reference solution is applied. Change work
//! goes in `reference.patch` (applied at the agent's working directory);
//! answer work goes in `reference.answer` (the operator-visible text).

use std::path::Path;

use anyhow::{Context, ensure};

use crate::grade::{self, GradeOutcome, ResultSource};
use crate::task::BenchTask;
use crate::workspace::{self, TrialLayout};

pub const REFERENCE_PATCH: &str = "reference.patch";
pub const REFERENCE_ANSWER: &str = "reference.answer";

#[derive(Debug, Clone, PartialEq)]
pub struct Verification {
    pub task: String,
    pub untouched: GradeOutcome,
    pub reference: GradeOutcome,
}

impl Verification {
    pub fn holds(&self) -> bool {
        !self.untouched.success && self.reference.success
    }
}

/// Grade the task twice under `root` (which must not exist yet).
pub async fn verify(task: &BenchTask, root: &Path) -> anyhow::Result<Verification> {
    ensure!(!root.exists(), "{} already exists", root.display());
    let untouched = grade_variant(task, &root.join("untouched"), false).await?;
    let reference = grade_variant(task, &root.join("reference"), true).await?;
    Ok(Verification {
        task: task.file.id.clone(),
        untouched,
        reference,
    })
}

async fn grade_variant(
    task: &BenchTask,
    root: &Path,
    with_reference: bool,
) -> anyhow::Result<GradeOutcome> {
    let layout = TrialLayout::under(root);
    workspace::prepare(task, &layout)?;
    let (label, answer) = if with_reference {
        ("reference", apply_reference(task, &layout)?)
    } else {
        ("untouched", String::new())
    };
    std::fs::write(layout.answer_file(), answer)?;
    grade::grade(task, &layout, label, &ResultSource::Workspace).await
}

/// Apply the reference patch to the prepared workspace and return the
/// reference answer (empty when the task has none).
pub fn apply_reference(task: &BenchTask, layout: &TrialLayout) -> anyhow::Result<String> {
    let patch = task.dir.join(REFERENCE_PATCH);
    let answer = task.dir.join(REFERENCE_ANSWER);
    ensure!(
        patch.is_file() || answer.is_file(),
        "task {} has neither {REFERENCE_PATCH} nor {REFERENCE_ANSWER}",
        task.file.id
    );
    if patch.is_file() {
        let cwd = layout.agent_cwd(task);
        let patch_text = patch.to_string_lossy();
        workspace::git(&cwd, &["apply", "--whitespace=nowarn", &patch_text])
            .with_context(|| format!("applying {} in {}", patch.display(), cwd.display()))?;
    }
    if !answer.is_file() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&answer).with_context(|| format!("reading {}", answer.display()))
}
