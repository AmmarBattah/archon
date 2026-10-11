//! Shared test helpers: pipeline specs from inline TOML, model plans and
//! run variables.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use archon_core::catalog::PipelineSource;
use archon_core::id::{PipelineRunId, QueueItemId};
use archon_core::model::{Effort, ModelChoice, ModelPlan};
use archon_core::repo::ShipMode;
use archon_pipeline::spec::{PipelineFile, PipelineSpec};
use archon_pipeline::{Catalog, RunVars};

pub fn choice(model: &str) -> ModelChoice {
    ModelChoice {
        model: model.parse().expect("provider/model"),
        effort: Effort::High,
    }
}

/// A spec parsed from inline TOML; every stage gets a small default
/// prompt unless `prompts` overrides it.
pub fn spec_from(toml_text: &str, prompts: &[(&str, &str)]) -> PipelineSpec {
    let file: PipelineFile = toml::from_str(toml_text).expect("test TOML parses");
    let mut map: BTreeMap<String, String> = file
        .stages
        .iter()
        .map(|s| {
            (
                s.prompt.clone(),
                "Do {{task.title}} as {{stage.id}}.\n{{feedback}}".to_owned(),
            )
        })
        .collect();
    for (name, text) in prompts {
        map.insert((*name).to_owned(), (*text).to_owned());
    }
    PipelineSpec {
        file,
        source: PipelineSource::User,
        dir: None,
        prompts: map,
    }
}

pub fn builtin(name: &str) -> Arc<PipelineSpec> {
    let catalog = Catalog::builtin();
    Arc::new(catalog.get(name).expect("built-in exists").clone())
}

/// One choice per stage, and `fan_choices` choices for every fan-out stage.
pub fn plan_for(spec: &PipelineSpec, fan_choices: &[&str]) -> ModelPlan {
    let mut plan = BTreeMap::new();
    for stage in &spec.file.stages {
        let choices: Vec<ModelChoice> = if stage.fan_out > 1 && !fan_choices.is_empty() {
            fan_choices
                .iter()
                .take(stage.fan_out as usize)
                .map(|m| choice(m))
                .collect()
        } else {
            vec![choice("claude-code/opus")]
        };
        plan.insert(stage.id.clone(), choices);
    }
    ModelPlan(plan)
}

pub fn base_vars(run: &PipelineRunId, item: &QueueItemId) -> BTreeMap<String, String> {
    base_vars_merging(run, item, false)
}

/// The run worktree of a team pipeline: `main` under the item's directory.
pub const TEAM_WORKSPACE: &str = "/archon/worktrees/demo/w1/main";

/// As [`base_vars`], laid out as `spec` needs: a team pipeline runs in
/// [`TEAM_WORKSPACE`].
pub fn base_vars_for(
    spec: &PipelineSpec,
    run: &PipelineRunId,
    item: &QueueItemId,
) -> BTreeMap<String, String> {
    let mut vars = base_vars(run, item);
    let team = spec
        .file
        .stages
        .iter()
        .any(|s| s.kind == archon_core::catalog::StageKind::Team);
    if team {
        vars.insert("workspace".into(), TEAM_WORKSPACE.into());
    }
    vars
}

/// As [`base_vars`], with `ship.auto_merge` as the item's merge knob says.
pub fn base_vars_merging(
    run: &PipelineRunId,
    item: &QueueItemId,
    auto_merge: bool,
) -> BTreeMap<String, String> {
    RunVars {
        task_title: "Fix the widget",
        task_brief: "The widget breaks on empty input.",
        repo_name: Some("demo"),
        repo_path: Some(Path::new("/src/demo")),
        workspace: Path::new("/archon/worktrees/demo/w1"),
        branch: "archon/w1-fix-the-widget",
        base_branch: "main",
        ship_mode: ShipMode::Pr,
        auto_merge,
        run_id: run,
        item_id: item,
    }
    .to_map()
}
