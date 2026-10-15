//! Reviewers' experiment directories (ARCHITECTURE §13.5): Archon makes one
//! per item on its data disk before an agent that can review starts, names it
//! in the contract, and removes it when the item settles or, after a crash,
//! in a gc pass.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use archon_core::id::QueueItemId;
use archon_core::model::{Effort, ModelPlan};
use archon_core::ports::SpawnRequest;
use archon_core::protocol::{GcReport, Reply, Request};
use archon_core::queue::{QueueItem, QueueState};
use archon_daemon::{dispatcher, executor};
use common::{Harness, MODEL, choice, completed};

/// Only the review prompt of the harness's pipeline says this.
const REVIEWER: &str = "You are an independent senior reviewer";

fn experiments(h: &Harness, item: &QueueItemId) -> PathBuf {
    h.ctx.paths.experiments().join(item.as_str())
}

/// Whether `request`'s contract sends experiment copies into `dir`.
fn names(request: &SpawnRequest, dir: &Path) -> bool {
    let told = format!("`mktemp -d -p {}` copy", dir.display());
    request.instructions.contains(&told)
}

fn reviews(h: &Harness) -> Vec<SpawnRequest> {
    let requests = h.backend.requests().into_iter();
    requests
        .filter(|r| r.objective.contains(REVIEWER))
        .collect()
}

async fn cancel(h: &Harness, item: &QueueItem) {
    let cancel = Request::CancelItem {
        item: item.id.clone(),
        reason: None,
    };
    h.cli(cancel).await.expect("cancel");
    let state = h.state(&item.id).await;
    assert!(matches!(state, QueueState::Cancelled { .. }), "{state:?}");
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.map(|entry| entry.expect("entry").path()).collect()
}

/// Settle the implementer, then dispatch with the reviewers' spawns held, and
/// return once the item's directory exists while none was asked for yet.
async fn hold_reviewers_at_spawn(h: &Harness, dir: &Path) -> tokio::task::JoinHandle<()> {
    let gate = h.backend.stall_spawns(REVIEWER);
    h.settle_running(|_| completed("implemented")).await;
    let ctx = h.ctx.clone();
    let dispatch = tokio::spawn(async move {
        executor::coordination::reconcile(&ctx)
            .await
            .expect("reconcile");
        dispatcher::dispatch_once(&ctx).await.expect("dispatch");
    });
    let made = async {
        while !dir.is_dir() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), made)
        .await
        .expect("the directory is made while the reviewers' spawns wait");
    assert!(reviews(h).is_empty(), "made before any reviewer is created");
    gate.add_permits(2);
    dispatch
}

#[tokio::test]
async fn reviewers_find_the_directory_made_and_named_and_it_goes_when_the_item_settles() {
    let h = Harness::new().await;
    let item = h.enqueue("reviewed change").await;
    h.dispatch().await;
    let dir = experiments(&h, &item.id);
    assert!(
        !dir.exists(),
        "an implementer that cannot delegate reviews nothing"
    );

    let dispatch = hold_reviewers_at_spawn(&h, &dir).await;
    dispatch.await.expect("dispatch task");
    let reviews = reviews(&h);
    assert_eq!(reviews.len(), 2, "the pipeline fans out two reviewers");
    assert!(reviews.iter().all(|r| names(r, &dir)), "{:?}", reviews[0]);
    let implementer = &h.backend.requests()[0];
    assert!(
        !implementer.instructions.contains("mktemp"),
        "it runs no review"
    );

    std::fs::create_dir_all(dir.join("tmp.copy/src")).unwrap();
    std::fs::write(dir.join("tmp.copy/src/lib.rs"), "fn mutant() {}\n").unwrap();
    cancel(&h, &item).await;
    assert!(!dir.exists(), "removed when the item settled");
    assert_eq!(entries(&h.ctx.paths.experiments()), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn a_restarted_daemon_still_names_the_directory() {
    let mut h = Harness::new().await;
    let item = h.enqueue("survives a restart").await;
    h.dispatch().await;

    h.restart();
    executor::recover(&h.ctx).await.expect("recover");
    let dir = experiments(&h, &item.id);
    let dispatch = hold_reviewers_at_spawn(&h, &dir).await;
    dispatch.await.expect("dispatch task");
    let reviews = reviews(&h);
    assert_eq!(reviews.len(), 2);
    assert!(reviews.iter().all(|r| names(r, &dir)), "{:?}", reviews[0]);
}

fn delegating(name: &str, workspace: &str) -> String {
    format!(
        r#"[pipeline]
name = "{name}"
description = "one agent that may delegate its reviewers"
version = 1
workspace = "{workspace}"
requires_repo = false

[[stage]]
id = "implement"
kind = "work"
prompt = "implement.md"
timeout_minutes = 30
allow_delegation = true
"#
    )
}

fn implement_plan() -> ModelPlan {
    let mut plan = ModelPlan::default();
    let choices = vec![choice(MODEL, Effort::High)];
    plan.0.insert("implement".into(), choices);
    plan
}

#[tokio::test]
async fn a_delegating_agent_of_a_scratch_or_read_only_item_gets_its_own_directory() {
    let h = Harness::new().await;
    for (name, workspace) in [
        ("scratch-lead", "scratch"),
        ("readonly-lead", "repo-readonly"),
    ] {
        h.add_pipeline(name, &delegating(name, workspace));
        let item = h.enqueue_in(name, name, implement_plan()).await.unwrap();
        h.dispatch().await;
        let dir = experiments(&h, &item.id);
        assert!(dir.is_dir(), "{workspace}: made before the lead starts");
        let lead = h.backend.requests().pop().expect("the lead");
        assert!(
            lead.instructions.contains("\n## Delegation\n"),
            "{workspace}"
        );
        assert!(names(&lead, &dir), "{workspace}: {}", lead.instructions);

        let run = h.run(&item.id).await;
        let apart = !dir.starts_with(&run.workspace) && !run.workspace.starts_with(&dir);
        assert!(
            apart,
            "{workspace}: {} and {}",
            dir.display(),
            run.workspace.display()
        );
        cancel(&h, &item).await;
        assert!(!dir.exists(), "{workspace}: removed when the item settled");
        if workspace == "scratch" {
            assert!(run.workspace.is_dir(), "the scratch workspace waits for gc");
        }
    }
}

async fn gc(h: &Harness) -> GcReport {
    let request = Request::Gc {
        older_than_secs: None,
        force: false,
    };
    match h.cli(request).await {
        Ok(Reply::Gc(report)) => *report,
        other => panic!("gc: {other:?}"),
    }
}

#[tokio::test]
async fn gc_deletes_what_a_crash_left_of_settled_and_unknown_items_only() {
    let h = Harness::new().await;
    let live = h.enqueue("still running").await;
    h.dispatch().await;
    let done = h.enqueue("settled").await;
    cancel(&h, &done).await;
    let root = h.ctx.paths.experiments();
    let stranger = QueueItemId::new();
    let claimed = ".gc-1-left-by-a-stopped-pass-0";
    let names = [
        live.id.as_str(),
        done.id.as_str(),
        stranger.as_str(),
        "not-an-item",
        claimed,
    ];
    for name in names {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("probe.sh"), "exit 0\n").unwrap();
    }

    let report = gc(&h).await;
    for gone in [done.id.as_str(), stranger.as_str(), claimed] {
        assert!(!root.join(gone).exists(), "{gone} is collected");
        assert!(report.removed.contains(&root.join(gone)), "{report:?}");
    }
    assert!(
        root.join(live.id.as_str()).join("probe.sh").exists(),
        "a live item keeps its copies"
    );
    assert!(root.join("not-an-item").exists(), "not Archon's to judge");
    assert!(report.bytes_freed > 0, "{report:?}");
}
