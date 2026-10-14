//! Every bundled grader must fail on the untouched fixture and pass once the
//! task's reference solution is applied; optional naive solutions must fail.

use std::path::Path;

use archon_bench::task::BenchTask;
use archon_bench::verify::{Verification, verify};
use tokio::task::JoinSet;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_grader_fails_untouched_and_passes_with_its_reference() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tasks = BenchTask::load_all(&manifest.join("tasks")).unwrap();
    assert!(!tasks.is_empty(), "the suite is empty");
    let work = manifest.join(".work");
    std::fs::create_dir_all(&work).unwrap();
    let base = tempfile::Builder::new()
        .prefix("verify-")
        .tempdir_in(&work)
        .unwrap();
    let mut running = JoinSet::new();
    for task in tasks {
        let root = base.path().join(&task.file.id);
        running.spawn(async move { verify(&task, &root).await });
    }
    let mut broken: Vec<Verification> = Vec::new();
    let mut checked = 0;
    let mut naive_checked = 0;
    while let Some(joined) = running.join_next().await {
        let verification = joined.unwrap().unwrap();
        checked += 1;
        naive_checked += usize::from(verification.naive.is_some());
        if !verification.holds() {
            broken.push(verification);
        }
    }
    if !broken.is_empty() {
        let kept = base.keep();
        panic!(
            "graders broken (logs kept under {}):\n{broken:#?}",
            kept.display()
        );
    }
    assert!(checked >= 11, "only {checked} tasks were verified");
    assert!(
        naive_checked >= 3,
        "only {naive_checked} naive fixes were verified"
    );
}

#[test]
fn naive_quality_fixes_pass_the_visible_examples() {
    use archon_bench::verify::NAIVE_PATCH;
    use archon_bench::workspace::{self, TrialLayout};

    let suite = Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks");
    let base = tempfile::tempdir().unwrap();
    let cases = [
        ("quality-ledger-transfer", "test_ledger"),
        ("quality-safe-extract", "test_safe_extract"),
        ("quality-retry-budget", "test_retry_budget"),
    ];
    for (id, module) in cases {
        let task = BenchTask::load(&suite.join(id)).unwrap();
        let layout = TrialLayout::under(&base.path().join(id));
        workspace::prepare(&task, &layout).unwrap();
        let cwd = layout.agent_cwd(&task);
        workspace::git(
            &cwd,
            &["apply", task.dir.join(NAIVE_PATCH).to_str().unwrap()],
        )
        .unwrap();
        let output = std::process::Command::new("python3")
            .args(["-m", "unittest", "-q", module])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{id}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("\nOK\n"),
            "{id}: no passing test summary"
        );
    }
}
