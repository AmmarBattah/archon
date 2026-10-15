//! Fixed quality gate: complete paired evidence, priced trials, and no lost passes.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

use crate::metrics::{Arm, RunManifest, TrialResult};
use crate::summary;

pub const TASKS: [&str; 4] = [
    "fix-off-by-one",
    "quality-ledger-transfer",
    "quality-retry-budget",
    "review-seeded-bug",
];
pub const DEFAULT_MAX_USD: f64 = 7.0;
pub const ARMS: [Arm; 2] = [Arm::Archon, Arm::Aether];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub suite_version: u32,
    pub tasks: Vec<String>,
    pub model: String,
    pub effort: String,
    pub max_usd: f64,
    #[serde(default)]
    pub candidate: bool,
    pub archon_successes: u32,
    pub archon_trials: u32,
    pub source_run: String,
}

impl Baseline {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading smoke baseline {}", path.display()))?;
        let baseline: Self = serde_json::from_str(&text).context("parsing smoke baseline")?;
        baseline.validate()?;
        Ok(baseline)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.candidate,
            "candidate baseline requires review before use"
        );
        ensure!(self.suite_version == 1, "unsupported smoke suite version");
        ensure!(
            self.tasks == TASKS,
            "baseline does not describe the fixed smoke tasks"
        );
        ensure!(
            self.archon_trials == TASKS.len() as u32,
            "baseline needs {} Archon trials",
            TASKS.len()
        );
        ensure!(
            (1..=self.archon_trials).contains(&self.archon_successes),
            "baseline must contain a positive, valid pass count"
        );
        ensure!(
            !self.model.is_empty() && !self.effort.is_empty(),
            "baseline lacks model or effort"
        );
        ensure!(
            self.max_usd.is_finite() && self.max_usd > 0.0,
            "invalid baseline cost cap"
        );
        ensure!(
            !self.source_run.is_empty() && self.source_run != "pending",
            "baseline lacks a completed evidence run (template is not a baseline)"
        );
        Ok(())
    }
}

pub fn write_candidate(dir: &Path) -> anyhow::Result<()> {
    let results = summary::read_results(&dir.join("results.jsonl"))?;
    let manifest = summary::read_manifest(dir)?.context("bootstrap needs run.json")?;
    let mut baseline = Baseline {
        suite_version: 1,
        tasks: TASKS.map(str::to_owned).to_vec(),
        model: manifest
            .models
            .get("archon")
            .context("missing Archon model")?
            .clone(),
        effort: manifest.effort.clone(),
        max_usd: manifest.max_usd,
        candidate: false,
        archon_successes: 1,
        archon_trials: TASKS.len() as u32,
        source_run: manifest.stamp.clone(),
    };
    let gate = evaluate(&results, &manifest, &baseline, baseline.max_usd)?;
    baseline.archon_successes = gate.archon_successes;
    baseline.validate()?;
    baseline.candidate = true;
    let path = dir.join("candidate-baseline.json");
    std::fs::write(&path, serde_json::to_string_pretty(&baseline)?)
        .with_context(|| format!("writing candidate baseline {}", path.display()))?;
    println!("candidate baseline: {} (review required)", path.display());
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub passed: bool,
    pub archon_successes: u32,
    pub archon_trials: u32,
    pub baseline_successes: u32,
    pub total_cost_usd: f64,
}

pub fn check_directory(dir: &Path, baseline: &Baseline, max_usd: f64) -> anyhow::Result<Gate> {
    let results = summary::read_results(&dir.join("results.jsonl"))?;
    let manifest = summary::read_manifest(dir)?.context("smoke needs run.json")?;
    let gate = evaluate(&results, &manifest, baseline, max_usd)?;
    std::fs::write(dir.join("gate.json"), serde_json::to_string_pretty(&gate)?)?;
    println!(
        "smoke: {} — Archon {}/{}; baseline {}/{}; total {}${:.2}",
        if gate.passed { "PASS" } else { "FAIL" },
        gate.archon_successes,
        gate.archon_trials,
        gate.baseline_successes,
        gate.archon_trials,
        if manifest.spent_floor { "≥ " } else { "" },
        gate.total_cost_usd
    );
    ensure!(
        gate.passed,
        "Archon's pass rate fell below the stored smoke baseline"
    );
    Ok(gate)
}

pub fn evaluate(
    results: &[TrialResult],
    manifest: &RunManifest,
    baseline: &Baseline,
    max_usd: f64,
) -> anyhow::Result<Gate> {
    baseline.validate()?;
    ensure!(
        max_usd.is_finite() && max_usd > 0.0,
        "invalid smoke cost cap"
    );
    ensure!(
        max_usd <= baseline.max_usd,
        "smoke cost cap exceeds baseline cap"
    );
    validate_manifest(manifest, baseline)?;
    ensure!(
        results.len() == TASKS.len() * ARMS.len(),
        "smoke requires all {} results",
        TASKS.len() * ARMS.len()
    );
    let mut cells = BTreeSet::new();
    let mut total_cost_usd = 0.0;
    let mut archon_successes = 0;
    for result in results {
        validate_result(result, baseline)?;
        ensure!(
            cells.insert((&result.task, result.arm)),
            "duplicate smoke result"
        );
        let cost = result
            .effective_cost_usd()
            .context("unpriced smoke trial")?;
        ensure!(cost.is_finite() && cost >= 0.0, "invalid smoke trial cost");
        total_cost_usd += cost;
        archon_successes += u32::from(result.arm == Arm::Archon && result.success);
    }
    ensure!(
        total_cost_usd <= max_usd.min(manifest.max_usd),
        "smoke exceeded ${max_usd:.2}: ${total_cost_usd:.2}"
    );
    ensure!(
        (manifest.spent_usd - total_cost_usd).abs() < 0.000001,
        "run.json spend disagrees with results.jsonl"
    );
    assert_eq!(cells.len(), TASKS.len() * ARMS.len());
    assert!(archon_successes <= baseline.archon_trials);
    Ok(Gate {
        passed: archon_successes >= baseline.archon_successes,
        archon_successes,
        archon_trials: baseline.archon_trials,
        baseline_successes: baseline.archon_successes,
        total_cost_usd,
    })
}

fn validate_manifest(manifest: &RunManifest, baseline: &Baseline) -> anyhow::Result<()> {
    ensure!(
        manifest.tasks == baseline.tasks,
        "run used different smoke tasks"
    );
    ensure!(manifest.arms == ARMS, "smoke needs both arms");
    ensure!(
        manifest.trials == 1 && manifest.parallel == 1,
        "smoke requires one serial trial per cell"
    );
    ensure!(manifest.timeout_factor == 1.0, "smoke time limits changed");
    ensure!(
        manifest.planned == TASKS.len() * ARMS.len()
            && manifest.completed == manifest.planned
            && manifest.skipped == 0,
        "incomplete smoke run; budget stops cannot pass"
    );
    ensure!(
        manifest.max_usd.is_finite() && manifest.max_usd > 0.0,
        "invalid run cost cap"
    );
    ensure!(
        manifest.max_usd <= baseline.max_usd,
        "run cost cap exceeds baseline cap"
    );
    ensure!(
        manifest.finished_at.is_some() && manifest.stopped_reason.is_none(),
        "smoke did not finish"
    );
    ensure!(
        manifest.unpriced_trials == 0 && manifest.spent_usd.is_finite(),
        "unpriced or invalid spend"
    );
    ensure!(
        manifest.effort == baseline.effort,
        "effort differs from baseline"
    );
    for arm in ARMS {
        ensure!(
            manifest.models.get(arm.name()) == Some(&baseline.model),
            "{arm} model differs from baseline"
        );
    }
    Ok(())
}

fn validate_result(result: &TrialResult, baseline: &Baseline) -> anyhow::Result<()> {
    ensure!(
        TASKS.contains(&result.task.as_str()),
        "unexpected smoke task {}",
        result.task
    );
    ensure!(ARMS.contains(&result.arm), "unexpected smoke arm");
    ensure!(result.trial == 1, "smoke trial number must be 1");
    ensure!(
        result.error.is_none(),
        "smoke infrastructure error in {}: {:?}",
        result.task,
        result.error
    );
    ensure!(!result.timed_out, "smoke timed out on {}", result.task);
    ensure!(
        result.model == baseline.model,
        "trial model differs from baseline"
    );
    ensure!(
        result.effort.as_ref() == Some(&baseline.effort),
        "trial effort differs from baseline"
    );
    ensure!(
        result.wall_secs.is_finite() && result.wall_secs >= 0.0,
        "invalid trial wall time"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
