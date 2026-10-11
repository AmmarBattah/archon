//! Shared scaffolding for the fake-server tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::time::Duration;

use archon_aether::ClientOptions;
use archon_aether::client::AetherClient;
use archon_aether::fake::{self, FakeAether};
use serde_json::{Value, json};
use tempfile::TempDir;

pub const AGENT: &str = "agt_01ARZ3NDEKTSV4RRFFQ69G5FAV";
pub const AGENT_2: &str = "agt_01BX5ZZKBKACTAV9WEVGEMMVRZ";
pub const TASK: &str = "tsk_01ARZ3NDEKTSV4RRFFQ69G5FAV";
pub const RUN: &str = "run_01ARZ3NDEKTSV4RRFFQ69G5FAV";

pub const WAIT: Duration = Duration::from_secs(5);

/// A fake daemon in its own temp directory; keep the `TempDir` alive.
pub async fn fake_aether() -> (TempDir, FakeAether) {
    let dir = tempfile::tempdir().expect("temp dir");
    let fake = FakeAether::start(&dir.path().join("aetherd.sock"))
        .await
        .expect("fake aetherd binds");
    (dir, fake)
}

pub async fn connect(fake: &FakeAether) -> AetherClient {
    connect_with(fake, ClientOptions::default()).await
}

pub async fn connect_with(fake: &FakeAether, options: ClientOptions) -> AetherClient {
    AetherClient::connect_with(&fake.endpoint(), options)
        .await
        .expect("connects to the fake")
}

/// A `task.completed` event for `AGENT`/`TASK`.
pub fn completed(summary: &str) -> Value {
    fake::event(
        Some(AGENT),
        Some(TASK),
        json!({"type": "task.completed", "result": {"summary": summary, "artifacts": []}}),
    )
}
