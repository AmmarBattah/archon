//! Stage messages and passive run-node inboxes share durable news delivery.

use archon_aether::events::InboxMessage;
use archon_core::aether_ids::AetherAgentId;
use archon_core::agent_message::{
    AgentMessage, MAX_AGENT_MESSAGES, MessageKind, MessageSender, check_text,
};
use archon_core::error::ArchonError;
use archon_core::notify::FeedItem;
use archon_core::protocol::Reply;
use archon_core::run::StageRun;
use archon_queue::Store;
use serde::{Deserialize, Serialize};

use crate::context::DaemonContext;
use crate::{executor, feed, triggers};

#[derive(Serialize, Deserialize)]
struct OperatorMessage {
    seq: u64,
    text: String,
}

fn operator_key(agent: &AetherAgentId) -> String {
    format!("message_operator:{agent}")
}

fn counter_key(sender: &MessageSender, target: &AetherAgentId) -> String {
    match sender {
        MessageSender::Agent { id } => format!("message_count:{id}"),
        MessageSender::Operator | MessageSender::System => {
            format!("message_count:{target}:external")
        }
    }
}

fn read<T: serde::de::DeserializeOwned>(
    store: &Store,
    key: &str,
) -> Result<Option<T>, ArchonError> {
    store
        .kv_get(key)?
        .map(|raw| {
            serde_json::from_str(&raw).map_err(|e| ArchonError::Store(format!("kv {key}: {e}")))
        })
        .transpose()
}

fn next_count(store: &Store, key: &str) -> Result<u32, ArchonError> {
    let count: u32 = read(store, key)?.unwrap_or(0);
    if count >= MAX_AGENT_MESSAGES {
        return Err(ArchonError::Invalid(format!(
            "message_orchestrator limit reached ({MAX_AGENT_MESSAGES} messages per agent or run-node external inbox); this message was refused"
        )));
    }
    Ok(count + 1)
}

fn stage_fields(message: &mut AgentMessage, stage: &StageRun) {
    message.stage = Some(stage.stage_id.clone());
    message.stage_run = Some(stage.id.clone());
    message.attempt = Some(stage.attempt);
}

pub(crate) async fn send(
    ctx: &DaemonContext,
    caller: Option<&AetherAgentId>,
    kind: MessageKind,
    text: String,
) -> Result<Reply, ArchonError> {
    check_text(&text)?;
    let agent = caller.ok_or_else(|| {
        ArchonError::Forbidden(
            "message_orchestrator requires Aether caller metadata (send_caller_meta = true)".into(),
        )
    })?;
    let receipt = {
        let mut store = ctx.store.lock().await;
        let stage = store.stage_run_by_agent(agent)?.ok_or_else(|| {
            ArchonError::Forbidden(format!(
                "message_orchestrator is for recorded stage agents; {agent} is not one. If your stage just started, retry shortly while continuing your work"
            ))
        })?;
        if stage.state.is_terminal() {
            return Err(ArchonError::Forbidden(
                "this stage has already settled".into(),
            ));
        }
        let run = store.run(&stage.run)?;
        let sender = MessageSender::Agent { id: agent.clone() };
        let key = counter_key(&sender, agent);
        let count = next_count(&store, &key)?.to_string();
        let operator: Option<OperatorMessage> = read(&store, &operator_key(agent))?;
        let original_sender =
            if kind == MessageKind::Relay && operator.is_some_and(|m| m.text == text) {
                MessageSender::Operator
            } else {
                sender
            };
        let mut message = AgentMessage {
            item: run.item,
            run: run.id,
            stage: None,
            stage_run: None,
            attempt: None,
            agent: Some(agent.clone()),
            kind,
            text,
            original_sender,
            addressed_to_run_node: None,
        };
        stage_fields(&mut message, &stage);
        triggers::record_message(&mut store, message, &[(&key, &count)])?
    };
    publish(ctx, &receipt);
    Ok(Reply::MessageReceipt {
        receipt: receipt.id,
    })
}

fn publish(ctx: &DaemonContext, receipt: &FeedItem) {
    feed::publish(ctx, vec![receipt.clone()]);
    triggers::wake(ctx);
}

pub(crate) async fn inbox(
    ctx: &DaemonContext,
    target: &AetherAgentId,
    seq: u64,
    incoming: &InboxMessage,
) -> Result<(), ArchonError> {
    let cursor = format!("message_inbox:{target}");
    let known = {
        let store = ctx.store.lock().await;
        let seen: u64 = read(&store, &cursor)?.unwrap_or(0);
        if seq <= seen {
            return Ok(());
        }
        executor::node_run(&store, target)?.is_some()
            || store.stage_run_by_agent(target)?.is_some()
            || store
                .kv_get(archon_queue::store::keys::ORCHESTRATOR_AGENT)?
                .as_deref()
                == Some(target.as_str())
    };
    if !known {
        match executor::recover_message_node(ctx, target).await {
            Ok(()) => {}
            Err(error) if error.is_store() || matches!(error, ArchonError::Unavailable(_)) => {
                return Err(error);
            }
            Err(refusal) => {
                refuse_inbox(ctx, target, seq, incoming, &refusal).await?;
                ctx.store.lock().await.kv_set(&cursor, &seq.to_string())?;
                return Ok(());
            }
        }
    }
    let result = {
        let mut store = ctx.store.lock().await;
        let seen: u64 = read(&store, &cursor)?.unwrap_or(0);
        if seq <= seen {
            return Ok(());
        }
        let Some(run) = executor::node_run(&store, target)? else {
            remember_operator(&mut store, target, seq, incoming)?;
            return Ok(());
        };
        let key = counter_key(&incoming.sender, target);
        match check_text(&incoming.text).and_then(|()| next_count(&store, &key)) {
            Ok(count) => {
                let count = count.to_string();
                let seq = seq.to_string();
                node_message(&store, target, run, incoming).and_then(|message| {
                    triggers::record_message(
                        &mut store,
                        message,
                        &[(&key, &count), (&cursor, &seq)],
                    )
                })
            }
            Err(refusal) => Err(refusal),
        }
    };
    match result {
        Ok(receipt) => publish(ctx, &receipt),
        Err(error) if error.is_store() => return Err(error),
        Err(refusal) => {
            refuse_inbox(ctx, target, seq, incoming, &refusal).await?;
            ctx.store.lock().await.kv_set(&cursor, &seq.to_string())?;
        }
    }
    Ok(())
}

fn remember_operator(
    store: &mut Store,
    target: &AetherAgentId,
    seq: u64,
    incoming: &InboxMessage,
) -> Result<(), ArchonError> {
    if incoming.sender != MessageSender::Operator || store.stage_run_by_agent(target)?.is_none() {
        return Ok(());
    }
    let key = operator_key(target);
    let old: Option<OperatorMessage> = read(store, &key)?;
    if old.is_some_and(|m| seq <= m.seq) {
        return Ok(());
    }
    let text = if check_text(&incoming.text).is_ok() {
        incoming.text.clone()
    } else {
        String::new()
    };
    store.kv_set(
        &key,
        &serde_json::to_string(&OperatorMessage { seq, text })?,
    )
}

fn node_message(
    store: &Store,
    target: &AetherAgentId,
    run: archon_core::id::PipelineRunId,
    incoming: &InboxMessage,
) -> Result<AgentMessage, ArchonError> {
    let mut message = AgentMessage {
        item: store.run(&run)?.item,
        run,
        stage: None,
        stage_run: None,
        attempt: None,
        agent: None,
        kind: if incoming.sender == MessageSender::Operator {
            MessageKind::Relay
        } else {
            MessageKind::Info
        },
        text: incoming.text.clone(),
        original_sender: incoming.sender.clone(),
        addressed_to_run_node: Some(target.clone()),
    };
    if let MessageSender::Agent { id } = &incoming.sender {
        message.agent = Some(id.clone());
        if let Some(stage) = store.stage_run_by_agent(id)?
            && stage.run == message.run
        {
            stage_fields(&mut message, &stage);
        }
    }
    Ok(message)
}

async fn refuse_inbox(
    ctx: &DaemonContext,
    target: &AetherAgentId,
    seq: u64,
    incoming: &InboxMessage,
    refusal: &ArchonError,
) -> Result<(), ArchonError> {
    let text = format!(
        "Message to passive Archon run node {target} was refused: {refusal}. Use message_orchestrator for bounded stage messages and continue your stage."
    );
    if let MessageSender::Agent { id } = &incoming.sender {
        match ctx
            .backend
            .send_message(id, &text, &format!("message_refusal:{target}:{seq}"))
            .await
        {
            Ok(()) => {}
            Err(error) if error.is_store() || matches!(error, ArchonError::Unavailable(_)) => {
                return Err(error);
            }
            Err(error) => {
                feed::system(
                    ctx,
                    format!("{text} Could not deliver the refusal to {id}: {error}"),
                )
                .await?;
            }
        }
    } else {
        feed::system(ctx, text).await?;
    }
    Ok(())
}
