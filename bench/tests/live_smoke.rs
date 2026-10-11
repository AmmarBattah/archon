//! One paid trial end to end: `fix-off-by-one` on the claude arm, graded.
//! Run with `ARCHON_LIVE=1 cargo test -p archon-bench --test live_smoke -- --ignored`.

use std::path::Path;
use std::time::Duration;

use archon_bench::arms::{ArmSettings, TrialInput, cli};
use archon_bench::grade;
use archon_bench::isolation::Protected;
use archon_bench::plan;
use archon_bench::task::BenchTask;
use archon_bench::workspace;
use archon_core::model::{Effort, ModelRef};

#[tokio::test]
#[ignore = "spends real money; set ARCHON_LIVE=1 and pass --ignored"]
async fn claude_arm_solves_and_is_graded_on_fix_off_by_one() {
    if std::env::var_os("ARCHON_LIVE").is_none_or(|v| v != "1") {
        eprintln!("ARCHON_LIVE != 1: skipping the paid smoke test");
        return;
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let task = BenchTask::load(&manifest.join("tasks/fix-off-by-one")).unwrap();
    let logs_root = manifest.join(".work");
    std::fs::create_dir_all(&logs_root).unwrap();
    let root = tempfile::Builder::new()
        .prefix("live-smoke-")
        .tempdir_in(&logs_root)
        .unwrap()
        .keep();
    let protected = Protected::for_run(&manifest.join("tasks"), &root).unwrap();
    let run_dir = plan::default_work_dir()
        .unwrap()
        .join(workspace::unguessable_name().unwrap());
    protected.check_work_dir(&run_dir).unwrap();
    workspace::create_run_dir(&run_dir).unwrap();
    let layout = workspace::TrialLayout::for_trial(&run_dir, root.join("logs")).unwrap();
    workspace::prepare(&task, &layout).unwrap();
    let model = ModelRef::new("claude-code", "claude-opus-5-5");
    let settings = ArmSettings {
        claude_bin: "claude".into(),
        codex_bin: "codex".into(),
        archon_bin: "archon".into(),
        aether_socket: None,
        archon_model: model.clone(),
        aether_model: model,
        claude_model: "claude-opus-5-5".into(),
        codex_model: String::new(),
        effort: Effort::Medium,
        prices: Vec::new(),
        env: protected.scrub_env(std::env::vars_os()),
    };
    let input = TrialInput {
        task: &task,
        layout: &layout,
        settings: &settings,
        timeout: Duration::from_secs(60 * u64::from(task.file.timeout_minutes)),
        key: "live-smoke",
    };
    let outcome = cli::run_claude(&input).await;
    assert_eq!(outcome.error, None, "logs under {}", root.display());
    let usage = outcome.usage.expect("claude reported no usage");
    assert!(usage.input_tokens + usage.cached_input_tokens > 0 && usage.output_tokens > 0);
    assert!(usage.cost_usd.is_some(), "claude reports its own cost");
    std::fs::write(layout.answer_file(), &outcome.answer).unwrap();
    let graded = grade::grade(&task, &layout, "claude", &outcome.source)
        .await
        .unwrap();
    eprintln!(
        "live smoke: success={} wall={:.0}s cost={:?} (logs under {}, workspace {})",
        graded.success,
        outcome.wall_secs,
        usage.cost_usd,
        root.display(),
        layout.workspace.display()
    );
}
