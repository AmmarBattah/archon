//! Check that graders reject untouched and optional naive fixtures and accept
//! reference patches/answers. Every variant gets a fresh workspace.

use std::path::Path;

use anyhow::{Context, ensure};

use crate::grade::{self, GradeOutcome, ResultSource};
use crate::task::BenchTask;
use crate::workspace::{self, TrialLayout};

pub const REFERENCE_PATCH: &str = "reference.patch";
pub const REFERENCE_ANSWER: &str = "reference.answer";
pub const NAIVE_PATCH: &str = "naive.patch";

#[derive(Debug, Clone, PartialEq)]
pub struct Verification {
    pub task: String,
    pub untouched: GradeOutcome,
    pub reference: GradeOutcome,
    pub naive: Option<GradeOutcome>,
}

impl Verification {
    pub fn holds(&self) -> bool {
        !self.untouched.success
            && self.reference.success
            && self.naive.as_ref().is_none_or(|outcome| !outcome.success)
    }
}

/// Grade untouched, reference, and optional naive variants in separate workspaces.
pub async fn verify(task: &BenchTask, root: &Path) -> anyhow::Result<Verification> {
    ensure!(!root.exists(), "{} already exists", root.display());
    let untouched = grade_variant(task, &root.join("untouched"), "untouched").await?;
    let reference = grade_variant(task, &root.join("reference"), "reference").await?;
    let naive = if task.dir.join(NAIVE_PATCH).try_exists()? {
        Some(grade_variant(task, &root.join("naive"), "naive").await?)
    } else {
        None
    };
    Ok(Verification {
        task: task.file.id.clone(),
        untouched,
        reference,
        naive,
    })
}

async fn grade_variant(task: &BenchTask, root: &Path, label: &str) -> anyhow::Result<GradeOutcome> {
    let layout = TrialLayout::under(root);
    workspace::prepare(task, &layout)?;
    let answer = match label {
        "reference" => apply_reference(task, &layout)?,
        "naive" => {
            apply_patch(task, &layout, NAIVE_PATCH)?;
            String::new()
        }
        "untouched" => String::new(),
        _ => anyhow::bail!("unknown verification variant {label:?}"),
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
        apply_patch(task, layout, REFERENCE_PATCH)?;
    }
    if !answer.is_file() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&answer).with_context(|| format!("reading {}", answer.display()))
}

fn apply_patch(task: &BenchTask, layout: &TrialLayout, name: &str) -> anyhow::Result<()> {
    let patch = task.dir.join(name);
    let cwd = layout.agent_cwd(task);
    ensure!(patch.is_file(), "{} is not a patch file", patch.display());
    ensure!(cwd.is_dir(), "{} is not prepared", cwd.display());
    workspace::git(
        &cwd,
        &["apply", "--whitespace=nowarn", &patch.to_string_lossy()],
    )
    .with_context(|| format!("applying {} in {}", patch.display(), cwd.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_naive_solutions_invalidate_the_grader() {
        let failed = GradeOutcome {
            success: false,
            score: None,
            note: None,
        };
        let passed = GradeOutcome {
            success: true,
            score: None,
            note: None,
        };
        let mut verification = Verification {
            task: "quality".into(),
            untouched: failed.clone(),
            reference: passed.clone(),
            naive: None,
        };
        assert!(verification.holds());
        verification.naive = Some(failed);
        assert!(verification.holds());
        verification.naive = Some(passed);
        assert!(!verification.holds());
    }
}
