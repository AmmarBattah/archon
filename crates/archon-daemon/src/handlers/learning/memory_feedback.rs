//! Persist intent before every Aether write, and recover lost receipts on undo.

use std::collections::BTreeSet;
use std::sync::Arc;

use archon_aether::AetherClient;
use archon_aether::memory_feedback::{
    MEMORY_CONTEXT_FEATURE, MEMORY_FEEDBACK_FEATURE, MemoryEvidence, RecordMemoryFeedback,
};
use archon_core::error::ArchonError;
use archon_core::feedback::{
    CorrectionRequest, MemoryFeedback, MemoryFeedbackEntry, MemoryFeedbackTask, OperatorFeedback,
    memory_feedback_key,
};
use archon_queue::Store;
use archon_queue::store::keys;

use crate::context::DaemonContext;

pub(super) fn plan(
    ctx: &DaemonContext,
    store: &Store,
    request: &CorrectionRequest,
) -> Result<MemoryFeedback, ArchonError> {
    let client = match ctx.aether.client() {
        Ok(client) => Some(client),
        Err(ArchonError::Unavailable(_)) => None,
        Err(error) => return Err(error),
    };
    let instance_id = store.kv_get(keys::AETHER_INSTANCE)?.unwrap_or_default();
    let tasks: BTreeSet<_> = store
        .stage_runs(&request.run)?
        .into_iter()
        .filter(|s| s.stage_id == request.stage)
        .filter_map(|s| s.aether_task)
        .collect();
    let skipped = if instance_id.is_empty() {
        Some("no Aether instance is known".into())
    } else if tasks.is_empty() {
        Some("the corrected stage has no recorded Aether tasks".into())
    } else if client.as_ref().is_some_and(|c| !supported(c)) {
        Some("Aether does not advertise memory_feedback and memory_context".into())
    } else {
        None
    };
    let ledger = MemoryFeedback {
        instance_id,
        skipped,
        tasks: tasks
            .into_iter()
            .map(|task| MemoryFeedbackTask {
                task,
                discovered: false,
                partial: false,
                skipped: None,
                error: None,
                records: Vec::new(),
            })
            .collect(),
    };
    ledger.check()?;
    assert!(ledger.tasks.windows(2).all(|t| t[0].task < t[1].task));
    assert!(ledger.skipped.is_some() || !ledger.instance_id.is_empty());
    Ok(ledger)
}

fn supported(client: &AetherClient) -> bool {
    client.supports_memory_feature(MEMORY_FEEDBACK_FEATURE)
        && client.supports_memory_feature(MEMORY_CONTEXT_FEATURE)
}

pub(super) async fn deliver(
    ctx: &DaemonContext,
    feedback: OperatorFeedback,
) -> Result<OperatorFeedback, ArchonError> {
    let Some(mut ledger) = feedback.memory_feedback.clone() else {
        return Ok(feedback);
    };
    if ledger.skipped.is_some() {
        return Ok(feedback);
    }
    assert!(ledger.check().is_ok());
    assert!(!ledger.instance_id.is_empty());
    for index in 0..ledger.tasks.len() {
        if !ledger.tasks[index].discovered {
            discover(ctx, &feedback, &mut ledger, index).await;
            save(ctx, &feedback, &ledger).await?;
        }
        for entry in 0..ledger.tasks[index].records.len() {
            deliver_entry(ctx, &feedback, &mut ledger, index, entry).await?;
        }
    }
    save(ctx, &feedback, &ledger).await
}

async fn discover(
    ctx: &DaemonContext,
    feedback: &OperatorFeedback,
    ledger: &mut MemoryFeedback,
    index: usize,
) {
    assert!(index < ledger.tasks.len());
    assert!(!ledger.tasks[index].discovered);
    let task = &mut ledger.tasks[index];
    if feedback.revoked_at.is_some() {
        task.discovered = true;
        task.skipped = Some("correction undone before context lookup".into());
        task.error = None;
        return;
    }
    let result = async {
        let client = bound_client(ctx, &ledger.instance_id)?;
        client.memories_in_context(&task.task).await
    }
    .await;
    match result {
        Ok(context) => {
            let memories: BTreeSet<_> = context
                .snapshots
                .into_iter()
                .flat_map(|s| s.memories)
                .collect();
            let named = feedback.curation.as_ref().map(|c| &c.memory);
            task.records = memories
                .into_iter()
                .filter(|m| named.is_none_or(|n| n == m))
                .map(|memory| MemoryFeedbackEntry {
                    memory,
                    feedback: None,
                    attempted: false,
                    retracted: false,
                    dropped: false,
                    error: None,
                })
                .collect();
            task.discovered = true;
            task.partial = context.truncated;
            task.error = None;
            if task.records.is_empty() {
                task.skipped = Some(
                    if named.is_some() {
                        "named memory was not listed in this task's context"
                    } else {
                        "no memories were listed in this task's context"
                    }
                    .into(),
                );
            }
        }
        Err(error) => task.error = Some(error.to_string()),
    }
}

async fn deliver_entry(
    ctx: &DaemonContext,
    feedback: &OperatorFeedback,
    ledger: &mut MemoryFeedback,
    task_index: usize,
    entry_index: usize,
) -> Result<(), ArchonError> {
    assert!(ledger.tasks[task_index].discovered);
    assert!(entry_index < ledger.tasks[task_index].records.len());
    let revoked = feedback.revoked_at.is_some();
    let entry = &mut ledger.tasks[task_index].records[entry_index];
    if entry.dropped || entry.retracted || (!revoked && entry.feedback.is_some()) {
        return Ok(());
    }
    if revoked && !entry.attempted {
        entry.dropped = true;
        entry.error = None;
        save(ctx, feedback, ledger).await?;
        return Ok(());
    }
    let client = match bound_client(ctx, &ledger.instance_id) {
        Ok(client) => client,
        Err(error) => {
            entry.error = Some(error.to_string());
            save(ctx, feedback, ledger).await?;
            return Ok(());
        }
    };
    entry.attempted = true;
    save(ctx, feedback, ledger).await?;
    let task_state = &mut ledger.tasks[task_index];
    let task = &task_state.task;
    let entry = &mut task_state.records[entry_index];
    let result = send(&client, feedback, task, entry).await;
    entry.error = result.err().map(|error| error.to_string());
    save(ctx, feedback, ledger).await?;
    Ok(())
}

async fn send(
    client: &AetherClient,
    feedback: &OperatorFeedback,
    task: &archon_core::aether_ids::AetherTaskId,
    entry: &mut MemoryFeedbackEntry,
) -> Result<(), ArchonError> {
    assert!(entry.attempted && !entry.dropped);
    assert!(!entry.retracted);
    if entry.feedback.is_none() {
        let record = client
            .record_memory_feedback(RecordMemoryFeedback {
                memory: entry.memory.clone(),
                verdict: "hurt".into(),
                evidence: if feedback.curation.is_some() {
                    MemoryEvidence::Operator
                } else {
                    MemoryEvidence::Outcome
                },
                task: task.clone(),
                reason: format!("archon {}: {}", feedback.id, feedback.reason),
                idempotency_key: memory_feedback_key(&feedback.id, &entry.memory, task),
            })
            .await?;
        entry.feedback = Some(record.id);
    }
    if feedback.revoked_at.is_some() {
        client
            .retract_memory_feedback(
                entry.feedback.as_ref().expect("receipt recovered"),
                &format!("archon {}: operator correction undone", feedback.id),
            )
            .await?;
        entry.retracted = true;
    }
    Ok(())
}

fn bound_client(ctx: &DaemonContext, instance: &str) -> Result<Arc<AetherClient>, ArchonError> {
    let client = ctx.aether.client()?;
    if client.welcome().instance_id != instance {
        return Err(ArchonError::Unavailable(format!(
            "memory feedback belongs to Aether instance {instance}; connected to {}",
            client.welcome().instance_id
        )));
    }
    assert!(!instance.is_empty(), "delivery is bound to an instance");
    assert_eq!(client.welcome().instance_id, instance);
    Ok(client)
}

async fn save(
    ctx: &DaemonContext,
    feedback: &OperatorFeedback,
    ledger: &MemoryFeedback,
) -> Result<OperatorFeedback, ArchonError> {
    ctx.store
        .lock()
        .await
        .update_memory_feedback(&feedback.id, ledger)
}
