//! Against the real `aetherd` on this machine. Read-only and free: hello,
//! `status`, `list_models`, a replaying `subscribe`, and `get_agent` /
//! `get_task` on ids those events name. Run with
//! `ARCHON_LIVE=1 cargo test -p archon-aether --test live -- --ignored`.

use std::time::Duration;

use archon_aether::client::{AetherClient, Endpoint};
use archon_aether::wire::{Command, EventQuery};
use archon_core::aether_ids::{AetherAgentId, AetherTaskId};
use archon_core::config::AetherConfig;

const REPLAYED: u64 = 200;

fn live() -> bool {
    std::env::var("ARCHON_LIVE").is_ok_and(|v| v == "1")
}

#[tokio::test]
#[ignore = "needs a running aetherd; set ARCHON_LIVE=1"]
async fn hello_list_models_and_subscribe_against_the_real_daemon() {
    if !live() {
        eprintln!("skipped: ARCHON_LIVE is not 1");
        return;
    }
    let endpoint = Endpoint::from_config(&AetherConfig::default()).unwrap();
    let client = AetherClient::connect(&endpoint)
        .await
        .expect("aetherd reachable");
    let welcome = client.welcome().clone();
    let spoken = archon_aether::wire::MIN_PROTOCOL_VERSION..=archon_aether::wire::PROTOCOL_VERSION;
    assert!(spoken.contains(&welcome.protocol_version), "{welcome:?}");
    eprintln!("welcome: {welcome:?}");

    let models = client.list_models().await.expect("list_models decodes");
    assert!(!models.is_empty(), "Aether lists at least one model");
    let names: Vec<String> = models.iter().map(|m| m.model.to_string()).collect();
    eprintln!("models: {names:?}");

    let (agent, task) = replay_recent_events(&client).await;
    if let Some(agent) = agent {
        let fetched = client.get_agent(&agent).await.expect("get_agent decodes");
        eprintln!("agent {} status {}", fetched.id, fetched.status);
    }
    if let Some(task) = task {
        let fetched = client.get_task(&task).await.expect("get_task decodes");
        let settled = archon_aether::backend::task_outcome(&fetched).is_some();
        eprintln!("task {} run {} settled {settled}", fetched.id, fetched.run);
    }
    client.ping().await.unwrap();
}

/// Replay the newest events through a subscription; every one must decode
/// and arrive in order. Returns the first agent and task ids seen.
async fn replay_recent_events(
    client: &AetherClient,
) -> (Option<AetherAgentId>, Option<AetherTaskId>) {
    let status = client.request(Command::Status, "status").await.unwrap();
    let last_seq = status["last_seq"]
        .as_u64()
        .expect("status carries last_seq");
    let after = last_seq.saturating_sub(REPLAYED);
    let mut sub = client
        .subscribe(Some(after), EventQuery::default())
        .await
        .unwrap();
    assert_eq!(sub.last_seq, after);
    let mut previous = after;
    let (mut agent, mut task) = (None, None);
    for _ in 0..(last_seq - after) {
        let event = tokio::time::timeout(Duration::from_secs(10), sub.next())
            .await
            .expect("replayed events arrive promptly")
            .expect("every real event decodes");
        assert!(event.seq > previous, "seq increases");
        previous = event.seq;
        agent = agent.or(event.agent.clone());
        task = task.or(event.task.clone());
    }
    eprintln!("replayed {} events up to seq {previous}", last_seq - after);
    (agent, task)
}
