//! Model policy for agents Archon did not create itself: children delegated
//! by a stage agent, the orchestrator, or another such child, at any depth.
//! A child outside the policy is cancelled and reported (ARCHITECTURE §7.4).
//! A child is owned before it is checked, so its spend, and its subtree's,
//! never depends on the check succeeding. A watched child is forgotten once
//! none of its events can still arrive.

use std::str::FromStr;

use archon_aether::AetherEvent;
use archon_aether::wire::Command;
use archon_core::Timestamp;
use archon_core::aether_ids::AetherAgentId;
use archon_core::config::RepoConfig;
use archon_core::error::ArchonError;
use archon_core::id::{PipelineRunId, QueueItemId};
use archon_core::model::{Effort, ModelChoice, ModelRef};
use archon_core::notify::NotificationKind;
use archon_core::policy::ModelPolicy;
use archon_core::protocol::RepoRef;

use super::fleet;
use crate::context::DaemonContext;
use crate::triggers::{self, TriggerEvent};
use crate::{executor, feed};

const CHILD_KEY_PREFIX: &str = "link_child_";
const STAGE_AGENT_KEY_PREFIX: &str = "link_stage_agent_";
/// Aether calls one pruning pass may make about the orchestrator's children.
const MAX_LIVENESS_CHECKS: usize = 32;

/// `kv`: a child Archon watches, holding its item as JSON.
pub(super) fn child_key(agent: &AetherAgentId) -> String {
    format!("{CHILD_KEY_PREFIX}{agent}")
}

/// `kv`: when a pruning pass first found an orchestrator's child ended.
fn ended_key(agent: &AetherAgentId) -> String {
    format!("link_ended_{agent}")
}

/// `kv`: set with `child_key` and cleared once the child's verdict is
/// reached, so a retried `agent.created` checks it again.
fn unchecked_key(agent: &AetherAgentId) -> String {
    format!("link_unchecked_{agent}")
}

/// `kv`: set once a child was cancelled for its model, so a replay does not
/// report it twice.
fn cancelled_key(agent: &AetherAgentId) -> String {
    format!("link_cancelled_{agent}")
}

/// `kv`: an agent Aether created under a run node, which is a stage agent,
/// holding its item. Its children are placed through it before the spawn
/// reply that records the stage agent has been handled.
pub(super) fn stage_agent_key(agent: &AetherAgentId) -> String {
    format!("{STAGE_AGENT_KEY_PREFIX}{agent}")
}

pub(super) async fn agent_created(
    ctx: &DaemonContext,
    event: &AetherEvent,
    agent: &AetherAgentId,
    model: &ModelRef,
) -> Result<(), ArchonError> {
    let Some(parent) = &event.parent else {
        return Ok(());
    };
    assert_ne!(parent, agent, "an agent is not its own parent");
    let node_run = executor::node_run(&*ctx.store.lock().await, parent)?;
    if let Some(run) = node_run {
        // Archon's own stage agent: its model was checked at spawn.
        return record_stage_agent(ctx, agent, &run).await;
    }
    let Some(item) = parent_item(ctx, parent).await? else {
        return Ok(());
    };
    {
        let mut store = ctx.store.lock().await;
        if let Some(run) = store.agent_run(parent)? {
            store.link_agent_run(agent, &run)?;
        }
    }
    if already_checked(ctx, agent).await? {
        return Ok(());
    }
    adopt(ctx, agent, item.as_ref()).await?;
    let policy = effective_policy(ctx, item.as_ref()).await?;
    match violation(ctx, agent, model, &policy).await? {
        Some(refusal) => cancel_child(ctx, agent, model, item, &refusal).await,
        None => ctx.store.lock().await.kv_delete(&unchecked_key(agent)),
    }
}

/// Every stage agent's `agent.created` precedes its children's in the
/// stream, so recording it here places them even when they are created
/// before `spawned_ok` records the stage.
async fn record_stage_agent(
    ctx: &DaemonContext,
    agent: &AetherAgentId,
    run: &PipelineRunId,
) -> Result<(), ArchonError> {
    let mut store = ctx.store.lock().await;
    let item = store.run(run)?.item;
    store.link_agent_run(agent, run)?;
    let key = stage_agent_key(agent);
    if let Some(recorded) = store.kv_get(&key)? {
        assert_eq!(recorded, item.as_str(), "a stage agent works for one item");
        return Ok(());
    }
    store.kv_set(&key, item.as_str())
}

/// The item a new child works for, when its parent is Archon's: the
/// orchestrator (no item), a stage agent, or a watched child.
async fn parent_item(
    ctx: &DaemonContext,
    parent: &AetherAgentId,
) -> Result<Option<Option<QueueItemId>>, ArchonError> {
    if let Some(owner) = fleet::owner(ctx, parent).await? {
        return Ok(Some(fleet::item_of(ctx, &owner).await?));
    }
    let orchestrator = crate::supervisor::current_orchestrator(ctx).await?;
    Ok((orchestrator.as_ref() == Some(parent)).then_some(None))
}

/// Stage agents were checked when Archon spawned them; a replayed
/// `agent.created` finds its first verdict. A child still `unchecked` has
/// none yet: its check failed with Aether unreachable and is retried.
async fn already_checked(ctx: &DaemonContext, agent: &AetherAgentId) -> Result<bool, ArchonError> {
    let store = ctx.store.lock().await;
    if store.stage_run_by_agent(agent)?.is_some() || store.kv_get(&cancelled_key(agent))?.is_some()
    {
        return Ok(true);
    }
    let owned = store.kv_get(&child_key(agent))?.is_some();
    Ok(owned && store.kv_get(&unchecked_key(agent))?.is_none())
}

/// Record the child as Archon's, pending its verdict.
async fn adopt(
    ctx: &DaemonContext,
    agent: &AetherAgentId,
    item: Option<&QueueItemId>,
) -> Result<(), ArchonError> {
    let raw = serde_json::to_string(&item)?;
    let mut store = ctx.store.lock().await;
    if let Some(owned) = store.kv_get(&child_key(agent))? {
        assert_eq!(owned, raw, "a child works for one item");
    }
    store.kv_set(&child_key(agent), &raw)?;
    store.kv_set(&unchecked_key(agent), "1")
}

/// Global policy narrowed by the item's repository, as for its stages.
async fn effective_policy(
    ctx: &DaemonContext,
    item: Option<&QueueItemId>,
) -> Result<ModelPolicy, ArchonError> {
    let global = ctx.config.models.policy();
    let Some(item) = item else {
        return Ok(global);
    };
    let repo = {
        let store = ctx.store.lock().await;
        match store.item(item)?.repo {
            Some(id) => Some(store.repo(&RepoRef(id.as_str().to_owned()))?),
            None => None,
        }
    };
    let Some(repo) = repo else {
        return Ok(global);
    };
    match RepoConfig::load(&repo.path) {
        Ok(config) => Ok(global.narrowed_by(&config.models.policy())),
        // The dispatcher refuses such a repo's new work; keep watching with
        // the global policy rather than stop watching.
        Err(error) => {
            tracing::warn!(repo = %repo.name, %error, "repo config unreadable; using global policy");
            Ok(global)
        }
    }
}

/// Why the child breaks `policy`, if it does. Effort is only in the
/// agent's config, so it is fetched only when a cap exists.
async fn violation(
    ctx: &DaemonContext,
    agent: &AetherAgentId,
    model: &ModelRef,
    policy: &ModelPolicy,
) -> Result<Option<ArchonError>, ArchonError> {
    let Some(cap) = policy.max_effort else {
        return Ok(model_refusal(policy, model));
    };
    let effort = match child_effort(ctx, agent).await? {
        ChildEffort::Known(effort) => effort,
        ChildEffort::Unknown => return Ok(model_refusal(policy, model)),
        // A cap the effort cannot be compared with is not met.
        ChildEffort::Unrecognised(raw) => {
            let refusal = ArchonError::Policy(format!(
                "effort {raw:?} is not one Archon can compare with the configured maximum {cap}"
            ));
            return Ok(model_refusal(policy, model).or(Some(refusal)));
        }
    };
    let choice = ModelChoice {
        model: model.clone(),
        effort,
    };
    Ok(policy.check(&choice).err())
}

fn model_refusal(policy: &ModelPolicy, model: &ModelRef) -> Option<ArchonError> {
    if policy.permits_model(model) {
        return None;
    }
    Some(ArchonError::Policy(format!(
        "model {model} is not allowed; allowed: {}",
        policy.describe_allowed()
    )))
}

/// What the child's config says about its effort.
#[derive(Debug, PartialEq)]
enum ChildEffort {
    Known(Effort),
    /// No effort in its config, or the agent is gone already (it ended, or
    /// this is a replay after it was reaped): only its model is checked.
    Unknown,
    Unrecognised(String),
}

async fn child_effort(
    ctx: &DaemonContext,
    agent: &AetherAgentId,
) -> Result<ChildEffort, ArchonError> {
    let client = ctx.aether.client()?;
    let command = Command::GetAgent { id: agent.clone() };
    let data = match client.request(command, "agent").await {
        Ok(data) => data,
        Err(ArchonError::NotFound(why)) => {
            tracing::info!(%agent, %why, "child gone before its effort was read; checking its model only");
            return Ok(ChildEffort::Unknown);
        }
        Err(error) => return Err(error),
    };
    let Some(raw) = data.pointer("/config/effort").and_then(|v| v.as_str()) else {
        return Ok(ChildEffort::Unknown);
    };
    Ok(match Effort::from_str(raw) {
        Ok(effort) => ChildEffort::Known(effort),
        Err(_) => ChildEffort::Unrecognised(raw.to_owned()),
    })
}

async fn cancel_child(
    ctx: &DaemonContext,
    agent: &AetherAgentId,
    model: &ModelRef,
    item: Option<QueueItemId>,
    refusal: &ArchonError,
) -> Result<(), ArchonError> {
    let client = ctx.aether.client()?;
    match client
        .cancel(agent, &format!("archon policy: {refusal}"))
        .await
    {
        Ok(()) | Err(ArchonError::NotFound(_) | ArchonError::InvalidTransition(_)) => {}
        Err(error) => return Err(error),
    }
    let text = format!("Cancelled agent {agent}: {refusal}");
    feed::notify(ctx, NotificationKind::Blocker, text, item, None).await?;
    let violation = TriggerEvent::PolicyViolation {
        agent: agent.to_string(),
        model: model.to_string(),
        action: "cancelled".into(),
    };
    triggers::push(ctx, violation).await?;
    // Still Archon's (`adopt`): its spend so far, and its subtree's, counts.
    let mut store = ctx.store.lock().await;
    store.kv_set(&cancelled_key(agent), "1")?;
    store.kv_delete(&unchecked_key(agent))
}

/// Forget watched children whose events can no longer arrive: those of an
/// item that finished before `cutoff`, and the orchestrator's once Aether
/// reported them ended before `cutoff`. A child's entry attributes its
/// spend and questions and lets its own children be checked, so it is kept
/// through the retention window. Returns how many were forgotten.
pub(crate) async fn prune_children(
    ctx: &DaemonContext,
    now: Timestamp,
    cutoff: Timestamp,
) -> Result<usize, ArchonError> {
    assert!(cutoff <= now, "the cutoff lies in the past");
    let mut checks_left = MAX_LIVENESS_CHECKS;
    let mut pruned = 0;
    for (agent, item) in watched_children(ctx).await? {
        let done = match &item {
            Some(item) => item_finished_before(ctx, item, cutoff).await?,
            None => ended_before(ctx, &agent, now, cutoff, &mut checks_left).await?,
        };
        if done {
            forget_child(ctx, &agent).await?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

async fn watched_children(
    ctx: &DaemonContext,
) -> Result<Vec<(AetherAgentId, Option<QueueItemId>)>, ArchonError> {
    let store = ctx.store.lock().await;
    let mut children = Vec::new();
    for (key, raw) in store.kv_prefix(CHILD_KEY_PREFIX)? {
        let id = key
            .strip_prefix(CHILD_KEY_PREFIX)
            .expect("ranged by prefix");
        let agent = AetherAgentId::parse(id)
            .map_err(|e| ArchonError::Store(format!("kv key {key}: {e}")))?;
        let item = serde_json::from_str(&raw)
            .map_err(|e| ArchonError::Store(format!("kv {key} = {raw:?}: {e}")))?;
        children.push((agent, item));
    }
    Ok(children)
}

/// The item finished before `cutoff`, or no longer exists.
async fn item_finished_before(
    ctx: &DaemonContext,
    item: &QueueItemId,
    cutoff: Timestamp,
) -> Result<bool, ArchonError> {
    match ctx.store.lock().await.item(item) {
        Ok(found) => Ok(found.state.is_terminal() && found.updated_at < cutoff),
        Err(ArchonError::NotFound(_)) => Ok(true),
        Err(error) => Err(error),
    }
}

/// Whether Aether reported the orchestrator's child ended before `cutoff`.
/// The first pass to find it ended records `now`; an unanswered question
/// is asked again by a later pass.
async fn ended_before(
    ctx: &DaemonContext,
    agent: &AetherAgentId,
    now: Timestamp,
    cutoff: Timestamp,
    checks_left: &mut usize,
) -> Result<bool, ArchonError> {
    let key = ended_key(agent);
    if let Some(raw) = ctx.store.lock().await.kv_get(&key)? {
        let ended: Timestamp = raw
            .parse()
            .map_err(|e| ArchonError::Store(format!("kv {key} = {raw:?}: {e}")))?;
        return Ok(ended < cutoff);
    }
    if *checks_left == 0 {
        return Ok(false);
    }
    *checks_left -= 1;
    match ctx.backend.is_live(agent).await {
        Ok(true) => {}
        Ok(false) => ctx.store.lock().await.kv_set(&key, &now.to_rfc3339())?,
        Err(error) if error.is_store() => return Err(error),
        Err(error) => {
            tracing::info!(%agent, %error, "cannot ask Aether about a watched child; asking next pass");
            *checks_left = 0;
        }
    }
    Ok(false)
}

async fn forget_child(ctx: &DaemonContext, agent: &AetherAgentId) -> Result<(), ArchonError> {
    let mut store = ctx.store.lock().await;
    store.kv_delete(&child_key(agent))?;
    store.kv_delete(&unchecked_key(agent))?;
    store.kv_delete(&cancelled_key(agent))?;
    store.kv_delete(&ended_key(agent))
}

/// Forget stage agents recorded at `agent.created` whose item finished
/// before `cutoff`. A recorded stage agent is also found through its stage
/// run, so the entry only matters while the item's events may still arrive.
/// Returns how many were forgotten.
pub(crate) async fn prune_stage_agents(
    ctx: &DaemonContext,
    cutoff: Timestamp,
) -> Result<usize, ArchonError> {
    let entries = ctx.store.lock().await.kv_prefix(STAGE_AGENT_KEY_PREFIX)?;
    let mut pruned = 0;
    for (key, raw) in entries {
        let id = key
            .strip_prefix(STAGE_AGENT_KEY_PREFIX)
            .expect("ranged by prefix");
        AetherAgentId::parse(id).map_err(|e| ArchonError::Store(format!("kv key {key}: {e}")))?;
        let item = QueueItemId::parse(&raw)
            .map_err(|e| ArchonError::Store(format!("kv {key} = {raw:?}: {e}")))?;
        if item_finished_before(ctx, &item, cutoff).await? {
            ctx.store.lock().await.kv_delete(&key)?;
            pruned += 1;
        }
    }
    Ok(pruned)
}
