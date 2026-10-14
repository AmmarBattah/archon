use super::*;
use crate::metrics::tests::result;

fn evidence() -> (Baseline, RunManifest, Vec<TrialResult>) {
    let baseline: Baseline =
        serde_json::from_str(include_str!("../../results-smoke/baseline.json")).unwrap();
    let mut manifest: RunManifest =
        serde_json::from_str(include_str!("../../results-v3/run.json")).unwrap();
    manifest.tasks = TASKS.map(str::to_owned).to_vec();
    manifest.arms = vec![Arm::Archon, Arm::Aether];
    manifest.trials = 1;
    manifest.parallel = 1;
    manifest.planned = 8;
    manifest.completed = 8;
    manifest.skipped = 0;
    manifest.spent_usd = 4.0;
    manifest.unpriced_trials = 0;
    manifest.stopped_reason = None;
    let mut results = Vec::new();
    for task in TASKS {
        for arm in [Arm::Archon, Arm::Aether] {
            let mut trial = result(Some(0.5), None);
            trial.task = task.to_owned();
            trial.arm = arm;
            trial.model = baseline.model.clone();
            trial.effort = Some(baseline.effort.clone());
            results.push(trial);
        }
    }
    assert_eq!(results.len(), manifest.planned);
    assert!(manifest.finished_at.is_some());
    (baseline, manifest, results)
}

#[test]
fn gate_detects_lost_archon_pass_without_penalizing_aether_failures() {
    let (baseline, manifest, mut results) = evidence();
    let first = evaluate(&results, &manifest, &baseline, 7.0).unwrap();
    assert!(first.passed);
    results[1].success = false;
    assert!(
        evaluate(&results, &manifest, &baseline, 7.0)
            .unwrap()
            .passed
    );
    for result in results.iter_mut().filter(|r| r.arm == Arm::Archon) {
        result.success = false;
    }
    let failed = evaluate(&results, &manifest, &baseline, 7.0).unwrap();
    assert!(!failed.passed);
    assert_eq!(failed.archon_successes, 0);
}

#[test]
fn incomplete_duplicate_and_foreign_cells_are_rejected() {
    let (baseline, manifest, results) = evidence();
    assert!(evaluate(&results[..7], &manifest, &baseline, 7.0).is_err());
    let mut duplicate = results.clone();
    duplicate[7] = duplicate[0].clone();
    assert!(evaluate(&duplicate, &manifest, &baseline, 7.0).is_err());
    let mut foreign = results;
    foreign[0].task = "not-smoke".into();
    assert!(evaluate(&foreign, &manifest, &baseline, 7.0).is_err());
}

#[test]
fn infrastructure_errors_timeouts_and_unpriced_trials_cannot_pass() {
    let (baseline, manifest, results) = evidence();
    let mut infra = results.clone();
    infra[0].error = Some("provider unavailable".into());
    assert!(evaluate(&infra, &manifest, &baseline, 7.0).is_err());
    let mut timed_out = results.clone();
    timed_out[0].timed_out = true;
    assert!(evaluate(&timed_out, &manifest, &baseline, 7.0).is_err());
    let mut unpriced = results;
    unpriced[0].cost_usd = None;
    assert!(evaluate(&unpriced, &manifest, &baseline, 7.0).is_err());
}

#[test]
fn budget_overruns_and_manifest_drift_cannot_pass() {
    let (baseline, mut manifest, results) = evidence();
    assert!(evaluate(&results, &manifest, &baseline, 3.99).is_err());
    assert!(evaluate(&results, &manifest, &baseline, f64::NAN).is_err());
    manifest.skipped = 1;
    assert!(evaluate(&results, &manifest, &baseline, 7.0).is_err());
    manifest.skipped = 0;
    manifest.spent_usd = 0.0;
    assert!(evaluate(&results, &manifest, &baseline, 7.0).is_err());
}

#[test]
fn changed_models_effort_and_baseline_shape_need_explicit_rebaselining() {
    let (mut baseline, manifest, mut results) = evidence();
    results[0].model = "another/model".into();
    assert!(evaluate(&results, &manifest, &baseline, 7.0).is_err());
    results[0].model = baseline.model.clone();
    results[0].effort = Some("low".into());
    assert!(evaluate(&results, &manifest, &baseline, 7.0).is_err());
    baseline.archon_successes = 5;
    assert!(baseline.validate().is_err());
    baseline.archon_successes = 0;
    assert!(baseline.validate().is_err());
}

#[test]
fn failing_gate_is_persisted_and_returns_an_error() {
    let (baseline, manifest, mut results) = evidence();
    for result in &mut results {
        result.success = false;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("run.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    let lines: Vec<_> = results
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    std::fs::write(dir.path().join("results.jsonl"), lines.join("\n")).unwrap();
    assert!(check_directory(dir.path(), &baseline, 7.0).is_err());
    let gate: Gate =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("gate.json")).unwrap())
            .unwrap();
    assert!(!gate.passed);
}
