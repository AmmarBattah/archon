//! The built-in `team-pr` pipeline against the scripted backend: a plan,
//! a backend and frontend team, an end-to-end test that fails and sends
//! its findings back to the team, a second build on top of the first, then
//! a passing test, one review and a ship, all on one run branch.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use archon_core::model::{Effort, ModelPlan};
use archon_core::queue::QueueState;
use archon_core::run::{PipelineRun, StageOutcome, StageRun};
use archon_pipeline::team::worker_branch;
use common::{Harness, MODEL, choice, completed, git, verdict};

const ROLES: [&str; 2] = ["backend", "frontend"];

/// One agent for each stage but `build`, which has one per role.
fn team_pr_plan() -> ModelPlan {
    let mut stages = BTreeMap::new();
    for stage in ["plan", "e2e", "review", "ship"] {
        stages.insert(stage.to_owned(), vec![choice(MODEL, Effort::Medium)]);
    }
    stages.insert("build".into(), vec![choice(MODEL, Effort::High); 2]);
    ModelPlan(stages)
}

fn commit_file(tree: &Path, file: &str, text: &str) {
    if let Some(dir) = Path::new(file).parent() {
        std::fs::create_dir_all(tree.join(dir)).expect("mkdir");
    }
    std::fs::write(tree.join(file), text).expect("write");
    git(tree, &["add", file]);
    git(tree, &["commit", "--quiet", "-m", &format!("write {file}")]);
}

fn role_tree(run: &PipelineRun, role: &str) -> PathBuf {
    let root = run.team_root.as_ref().expect("a team run");
    root.join(format!("build-{role}"))
}

/// The running `build` roles, in role order, all at `attempt`.
async fn running_builds(h: &Harness, attempt: u32) -> Vec<StageRun> {
    let mut running = h.running().await;
    running.sort_by_key(|s| s.fan_index);
    let shape: Vec<(&str, u32)> = running
        .iter()
        .map(|s| (s.stage_id.as_str(), s.attempt))
        .collect();
    assert_eq!(shape, vec![("build", attempt); 2], "{running:?}");
    running
}

/// The only running stage, which must be `stage` at `attempt`.
async fn running_one(h: &Harness, stage: &str, attempt: u32) {
    let running = h.running().await;
    assert_eq!(running.len(), 1, "{running:?}");
    let now = (running[0].stage_id.as_str(), running[0].attempt);
    assert_eq!(now, (stage, attempt));
}

/// Each role commits `<role>-v<attempt>.txt` on its branch and completes.
async fn build_round(h: &Harness, run: &PipelineRun, attempt: u32) {
    let running = running_builds(h, attempt).await;
    for role in ROLES {
        let tree = role_tree(run, role);
        let branch = worker_branch(&run.branch, &run.id, role, attempt);
        assert_eq!(git(&tree, &["rev-parse", "--abbrev-ref", "HEAD"]), branch);
        assert!(
            tree.join(".archon/plan.md").exists(),
            "{role} reads the plan"
        );
        commit_file(&tree, &format!("{role}-v{attempt}.txt"), "work\n");
    }
    let settled =
        h.settle_running(|s| completed(&format!("{} built", ROLES[s.fan_index as usize])));
    assert_eq!(settled.await.len(), running.len());
    h.dispatch().await;
}

fn e2e_failure() -> StageOutcome {
    let finding = "login form posts to /api/session, which the backend never serves";
    StageOutcome::Completed {
        summary: "ran it end to end\nVERDICT: fail".into(),
        structured: Some(serde_json::json!({
            "verdict": "fail",
            "findings": [{"severity": "high", "title": finding, "detail": "404 on submit"}],
        })),
    }
}

/// Build attempt 2 is told the end-to-end findings, and each role forks
/// from the run branch that already holds both roles' attempt 1.
async fn assert_rebuild_starts_from_attempt_one(h: &Harness, run: &PipelineRun) {
    running_builds(h, 2).await;
    let requests = h.backend.requests();
    let rebuild = requests.last().expect("build attempt 2");
    assert!(
        rebuild.objective.contains("/api/session"),
        "{}",
        rebuild.objective
    );
    for role in ROLES {
        let tree = role_tree(run, role);
        for earlier in ROLES {
            let file = format!("{earlier}-v1.txt");
            assert!(tree.join(&file).exists(), "{role} starts from {file}");
        }
    }
}

/// Both rounds of both roles are merged with `--no-ff` onto the one run
/// branch that ships.
fn assert_both_rounds_on_the_run_branch(run: &PipelineRun) {
    for role in ROLES {
        for attempt in [1, 2] {
            let file = format!("{role}-v{attempt}.txt");
            assert!(
                run.workspace.join(&file).exists(),
                "{file} is on the run branch"
            );
        }
    }
    let merges = git(&run.workspace, &["log", "--merges", "--format=%s"]);
    assert_eq!(merges.lines().count(), 4, "two roles, twice: {merges}");
    let branch = git(&run.workspace, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(branch, run.branch, "one branch carries it all");
}

#[tokio::test]
async fn a_failed_end_to_end_test_loops_the_team_back_then_ships() {
    let h = Harness::new().await;
    let item = h
        .enqueue_in("team-pr", "sign in", team_pr_plan())
        .await
        .expect("team-pr is a built-in");
    h.dispatch().await;
    let run = h.run(&item.id).await;
    running_one(&h, "plan", 1).await;
    commit_file(
        &run.workspace,
        ".archon/plan.md",
        "backend: api\nfrontend: form\n",
    );
    h.settle_running(|_| completed("split into backend and frontend"))
        .await;
    h.dispatch().await;

    build_round(&h, &run, 1).await;
    running_one(&h, "e2e", 1).await;
    h.settle_running(|_| e2e_failure()).await;
    h.dispatch().await;

    assert_rebuild_starts_from_attempt_one(&h, &run).await;
    build_round(&h, &run, 2).await;

    running_one(&h, "e2e", 2).await;
    h.settle_running(|_| verdict(true)).await;
    h.dispatch().await;
    running_one(&h, "review", 1).await;
    h.settle_running(|_| verdict(true)).await;
    h.dispatch().await;
    running_one(&h, "ship", 1).await;
    assert_both_rounds_on_the_run_branch(&run);

    h.settle_running(|_| completed("opened a pull request"))
        .await;
    assert!(
        matches!(h.state(&item.id).await, QueueState::Succeeded { .. }),
        "{:?}",
        h.state(&item.id).await
    );
    assert_eq!(
        h.stages(&item.id).await.len(),
        9,
        "plan, 2x2 build, 2 e2e, review, ship"
    );
    assert!(!run.workspace.exists(), "a shipped run's worktree goes");
    let kept = git(&h.repo, &["branch", "--list", &run.branch]);
    assert!(!kept.is_empty(), "the run branch is kept");
}
