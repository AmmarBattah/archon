//! The `aether` arm: one plain root agent in the trial workspace, driven
//! over Aether's socket. Tokens and cost come from its latest
//! `agent.usage_updated` totals; operator-visible messages are its
//! assistant messages.

use std::time::{Duration, Instant};

use anyhow::Context;
use archon_aether::wire::{Command, EventQuery, Task, TaskStatus};
use archon_aether::{AetherClient, Endpoint};
use archon_core::aether_ids::{AetherAgentId, AetherTaskId};
use serde_json::Value;

use crate::arms::{ArmOutcome, TrialInput, aether_request};
use crate::grade::ResultSource;
use crate::metrics::Arm;
use crate::usage::aether_usage;

pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// How long to wait, after the task settles, for the agent's turn to end.
/// The agent writes its final message after `complete_task` returns.
const TURN_END_GRACE: Duration = Duration::from_secs(120);
const TURN_END_POLL: Duration = Duration::from_millis(500);
/// Aether agent states in which a turn may still be producing messages.
const TURN_RUNNING_STATES: &[&str] = &["created", "ready", "running"];
/// Events fetched per `get_events` page.
const EVENT_PAGE: usize = 500;

/// Always an explicit socket: Aether's own default resolution would reach
/// the operator's daemon.
pub fn endpoint(socket: &std::path::Path) -> Endpoint {
    assert!(socket.is_absolute(), "sandbox sockets are absolute");
    Endpoint::Unix(socket.to_path_buf())
}

pub async fn run_aether(input: &TrialInput<'_>) -> ArmOutcome {
    match drive(input).await {
        Ok(outcome) => outcome,
        Err(e) => ArmOutcome::failed(format!("{e:#}"), ResultSource::Workspace),
    }
}

async fn drive(input: &TrialInput<'_>) -> anyhow::Result<ArmOutcome> {
    let socket = &input.settings.private_sockets()?.aether;
    let client = AetherClient::connect(&endpoint(socket))
        .await
        .context("connecting to aetherd")?;
    let deadline = archon_core::now()
        + chrono::Duration::from_std(input.timeout).context("timeout out of range")?;
    let started = Instant::now();
    let agent = client
        .create_agent(aether_request(input, deadline))
        .await
        .context("create_agent")?;
    let task_id = agent
        .current_task
        .clone()
        .context("Aether created the agent without a task for its objective")?;
    let (task, timed_out) = wait_for_task(&client, &agent.id, &task_id, input.timeout).await?;
    wait_for_turn_end(&client, &agent.id).await?;
    let wall_secs = started.elapsed().as_secs_f64();
    let events = agent_events(&client, &agent.id).await?;
    let usage = aether_usage(&events)?.map(|u| input.settings.price(Arm::Aether, u));
    let messages = assistant_messages(&events, &agent.id);
    let (answer, error) = settle_text(&task.status, &messages, timed_out);
    Ok(ArmOutcome {
        usage,
        answer,
        operator_messages: u32::try_from(messages.len()).unwrap_or(u32::MAX),
        wall_secs,
        setup_secs: None,
        timed_out,
        error,
        source: ResultSource::Workspace,
    })
}

/// Poll `get_task` until it settles; past the timeout, cancel the agent and
/// read the state it settled in.
async fn wait_for_task(
    client: &AetherClient,
    agent: &AetherAgentId,
    task: &AetherTaskId,
    timeout: Duration,
) -> anyhow::Result<(Task, bool)> {
    assert!(!timeout.is_zero(), "a zero timeout cancels immediately");
    let deadline = Instant::now() + timeout;
    loop {
        let current = client.get_task(task).await.context("get_task")?;
        if is_settled(&current.status) {
            return Ok((current, false));
        }
        if Instant::now() >= deadline {
            client
                .cancel(agent, "archon-bench: trial timeout")
                .await
                .context("cancelling the timed-out agent")?;
            tokio::time::sleep(POLL_INTERVAL).await;
            let last = client
                .get_task(task)
                .await
                .context("get_task after cancel")?;
            return Ok((last, true));
        }
        tokio::time::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())))
            .await;
    }
}

/// Wait until the agent's last turn has ended, so its final message is in
/// the event log; gives up after [`TURN_END_GRACE`] and reads what is there.
async fn wait_for_turn_end(client: &AetherClient, agent: &AetherAgentId) -> anyhow::Result<()> {
    let deadline = Instant::now() + TURN_END_GRACE;
    loop {
        let current = client.get_agent(agent).await.context("get_agent")?;
        let state = current.status["state"]
            .as_str()
            .context("get_agent returned a status without a state")?;
        if !TURN_RUNNING_STATES.contains(&state) || Instant::now() >= deadline {
            return Ok(());
        }
        tokio::time::sleep(TURN_END_POLL).await;
    }
}

pub fn is_settled(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed { .. } | TaskStatus::Failed { .. } | TaskStatus::Cancelled { .. }
    )
}

/// Every usage and message event of the agent and anything under it.
pub async fn agent_events(
    client: &AetherClient,
    agent: &AetherAgentId,
) -> anyhow::Result<Vec<Value>> {
    let query = EventQuery {
        agent: Some(agent.clone()),
        types: vec!["agent.usage_updated".into(), "agent.message".into()],
        ..EventQuery::default()
    };
    fetch_events(client, query).await
}

pub async fn fetch_events(client: &AetherClient, query: EventQuery) -> anyhow::Result<Vec<Value>> {
    let mut events = Vec::new();
    let mut after = 0u64;
    loop {
        let command = Command::GetEvents {
            after,
            limit: EVENT_PAGE,
            query: query.clone(),
        };
        let page = client
            .request(command, "events")
            .await
            .context("get_events")?;
        let page = page
            .as_array()
            .context("get_events did not return an array")?
            .clone();
        let full = page.len() >= EVENT_PAGE;
        for event in page {
            let seq = event["seq"].as_u64().context("an event without seq")?;
            anyhow::ensure!(
                seq > after,
                "get_events went backwards: {seq} after {after}"
            );
            after = seq;
            events.push(event);
        }
        if !full {
            return Ok(events);
        }
    }
}

/// Non-empty assistant texts of `agent` itself, in order.
pub fn assistant_messages(events: &[Value], agent: &AetherAgentId) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["kind"]["type"] == "agent.message" && e["kind"]["role"] == "assistant")
        .filter(|e| e["agent_id"].as_str() == Some(agent.as_str()))
        .map(|e| message_text(&e["kind"]["content"]))
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The operator-visible answer (task result, then the agent's final
/// message) and a harness error when Aether, not the agent, ended the task.
pub fn settle_text(
    status: &TaskStatus,
    messages: &[String],
    timed_out: bool,
) -> (String, Option<String>) {
    let last = messages.last().cloned().unwrap_or_default();
    let join = |head: &str| {
        if last.is_empty() || head.contains(last.trim()) {
            head.to_owned()
        } else {
            format!("{head}\n\n{last}")
        }
    };
    match status {
        TaskStatus::Completed { result } => (join(&result.summary), None),
        TaskStatus::Failed { error, .. } => (join(error), None),
        TaskStatus::Cancelled { reason } if timed_out => {
            (join(reason.as_deref().unwrap_or("")), None)
        }
        TaskStatus::Cancelled { reason } => (
            join(""),
            Some(format!(
                "Aether cancelled the task: {}",
                reason.as_deref().unwrap_or("no reason")
            )),
        ),
        other => (join(""), Some(format!("the task never settled: {other:?}"))),
    }
}
