//! Sections the autonomy knobs append to a rendered stage prompt
//! (ARCHITECTURE §18.1, §18.3). They are appended after rendering, so a stage
//! summary is never parsed as a template.

use std::collections::BTreeMap;

use archon_core::catalog::StageKind;
use archon_core::run::GateSubject;

use crate::autonomy;
use crate::spec::{PipelineSpec, StageSpec};
use crate::template;

/// The `{{feedback}}` a stage re-entered from an operator gate receives.
pub(crate) fn operator_feedback(subject: GateSubject, feedback: &str) -> String {
    let feedback = feedback.trim();
    assert!(!feedback.is_empty(), "a revision always carries feedback");
    format!(
        "The operator reviewed the {} and asked for changes:\n\n{feedback}",
        subject.noun()
    )
}

/// What `stage` gets beyond its own template: the `feedback` it was entered
/// with when its template does not show it, and context from the summaries
/// of the stages passed so far (`previous`) and the gates approved.
pub(crate) fn context(
    spec: &PipelineSpec,
    stage: &StageSpec,
    feedback: &str,
    previous: &BTreeMap<String, String>,
    approved: &BTreeMap<String, Option<String>>,
) -> String {
    let summary_of = |id: &str| previous.get(&template::previous_variable(id));
    let mut out = unshown_feedback(&spec.prompts[&stage.prompt], feedback);
    if let Some(propose) = autonomy::propose_stage(spec).filter(|p| p.id != stage.id)
        && let Some(summary) = summary_of(&propose.id)
    {
        out.push_str(&direction(summary, approved.get(&propose.id)));
    }
    if stage.prompt == autonomy::REPORT_PROMPT {
        out.push_str(&earlier_summaries(spec, stage, previous));
    }
    if stage.kind == StageKind::Ship
        && let Some(report) = autonomy::change_report_stage(spec)
        && let Some(summary) = summary_of(&report.id)
    {
        out.push_str(&change_report(summary));
    }
    out
}

/// A loop or a revision must never lose its feedback: a template without
/// `{{feedback}}` gets it as a section of its own.
fn unshown_feedback(template: &str, feedback: &str) -> String {
    let feedback = feedback.trim();
    if feedback.is_empty() {
        return String::new();
    }
    let names = template::placeholders(template).expect("validated prompts parse");
    if names.iter().any(|n| n == "feedback") {
        return String::new();
    }
    format!("\n\n## Feedback to address\n\n{feedback}\n")
}

/// `approval` is `Some(note)` once the operator approved the proposal.
fn direction(summary: &str, approval: Option<&Option<String>>) -> String {
    let status = match approval {
        Some(_) => {
            "The operator approved this proposal. Follow it; where you must deviate, say where and why in your summary."
        }
        None => {
            "The operator did not review this proposal. Treat it as the plan unless the task says otherwise."
        }
    };
    let mut out = format!(
        "\n\n## Direction from the propose stage\n\n{status} The full proposal is \
         `.archon/reports/proposal/index.html` in the workspace; do not commit it.\n\n{}\n",
        summary.trim()
    );
    if let Some(Some(note)) = approval {
        out.push_str(&format!(
            "\nThe operator's note on approving: {}\n",
            note.trim()
        ));
    }
    out
}

/// The shared change-report prompt cannot name a pipeline's stages, so
/// every earlier summary is listed here, the proposal excepted.
fn earlier_summaries(
    spec: &PipelineSpec,
    stage: &StageSpec,
    previous: &BTreeMap<String, String>,
) -> String {
    let index = spec.stage_index(&stage.id).expect("stage of this pipeline");
    let mut out = String::from("\n\n## Earlier stages\n");
    for earlier in &spec.file.stages[..index] {
        if earlier.kind == StageKind::Propose {
            continue;
        }
        if let Some(summary) = previous.get(&template::previous_variable(&earlier.id)) {
            out.push_str(&format!("\n### `{}`\n\n{}\n", earlier.id, summary.trim()));
        }
    }
    out
}

fn change_report(summary: &str) -> String {
    format!(
        "\n\n## Change report\n\nA change report was written before shipping. Its summary:\n\n{}\n\n\
         When you write the pull request body, include this report under a `## Change report` \
         heading: the summary above and the key points of the page it names. Do not commit \
         the report page, and do not wait for anyone to read it.\n",
        summary.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_feedback_names_what_was_reviewed() {
        let text = operator_feedback(GateSubject::Report, "  add a test  ");
        assert!(text.starts_with("The operator reviewed the change report"));
        assert!(text.ends_with("add a test"));
    }

    #[test]
    fn feedback_is_appended_only_where_the_template_does_not_show_it() {
        let section = unshown_feedback("Fix {{task.title}}.", "  add a test \n");
        assert_eq!(section, "\n\n## Feedback to address\n\nadd a test\n");
        assert_eq!(unshown_feedback("Fix it.\n{{feedback}}", "add a test"), "");
        assert_eq!(unshown_feedback("Fix it.", " \n"), "");
    }

    #[test]
    fn an_approved_direction_carries_the_note() {
        let note = Some("keep the flag".to_owned());
        let text = direction("Use a trait.", Some(&note));
        assert!(text.contains("The operator approved this proposal"));
        assert!(text.contains("note on approving: keep the flag"));
        let unreviewed = direction("Use a trait.", None);
        assert!(unreviewed.contains("did not review"));
        assert!(!unreviewed.contains("note on approving"));
    }
}
