//! `archon learning`: an operator's correction of one stage of one finished
//! run, and the curation of the one Aether memory it names. The correction
//! is recorded first; Aether is called without the store lock.

use std::collections::BTreeMap;
use std::sync::Arc;

use archon_aether::AetherClient;
use archon_aether::memory::MemoryRecord;
use archon_core::catalog::StageKind;
use archon_core::error::ArchonError;
use archon_core::feedback::{
    CorrectionRequest, CurationState, FeedbackReceipt, FeedbackView, LearningWhy, MemoryBefore,
    MemoryCuration, MemoryView, OperatorFeedback,
};
use archon_core::id::{FeedbackId, QueueItemId, RepoId};
use archon_core::policy::ModelPolicy;
use archon_core::protocol::Reply;
use archon_core::run::PipelineRun;
use archon_queue::store::{CorrectionFacts, RouteQuery};

use crate::context::DaemonContext;
use crate::executor::frozen;
use crate::resolve::RepoEnv;

pub(super) async fn correct(
    ctx: &DaemonContext,
    request: CorrectionRequest,
) -> Result<Reply, ArchonError> {
    request.check()?;
    let aether_instance = match &request.forget_memory {
        Some(_) => memory_instance(ctx)?,
        None => None,
    };
    let facts = correction_facts(ctx, &request, aether_instance).await?;
    let (feedback, replayed) = ctx
        .store
        .lock()
        .await
        .record_operator_correction(&request, &facts)?;
    assert_eq!(feedback.run, request.run, "recorded for the named run");
    let feedback = match feedback.curation.as_ref().map(|c| c.state) {
        Some(CurationState::Pending | CurationState::Failed) => forget(ctx, feedback).await?,
        _ => feedback,
    };
    receipt(ctx, feedback, replayed).await
}

pub(super) async fn why(ctx: &DaemonContext, item: &QueueItemId) -> Result<Reply, ArchonError> {
    let feedback = {
        let store = ctx.store.lock().await;
        store.item(item)?;
        store.item_feedback(item)?
    };
    let mut views = Vec::with_capacity(feedback.len());
    for event in feedback {
        views.push(view(ctx, event).await?);
    }
    assert!(views.iter().all(|v| &v.feedback.item == item));
    Ok(Reply::LearningWhy(Box::new(LearningWhy {
        item: item.clone(),
        feedback: views,
    })))
}

/// Revokes the correction's model penalty; its memory curation stays as it is.
pub(super) async fn undo(ctx: &DaemonContext, id: &FeedbackId) -> Result<Reply, ArchonError> {
    let (feedback, replayed) = {
        let store = ctx.store.lock().await;
        if !store.learning_config()?.enabled {
            return Err(ArchonError::Invalid(
                "learning is disabled, so no correction is undone; enable [learning] first".into(),
            ));
        }
        let before = store.operator_feedback(id)?;
        let after = store.revoke_feedback(id, archon_core::now())?;
        assert!(
            after.revoked_at.is_some(),
            "an undone correction is revoked"
        );
        assert_eq!(
            before.curation, after.curation,
            "undo leaves the memory alone"
        );
        (after, before.revoked_at.is_some())
    };
    receipt(ctx, feedback, replayed).await
}

/// Retries only the memory forget of a correction whose forget did not apply.
pub(super) async fn retry_memory(
    ctx: &DaemonContext,
    id: &FeedbackId,
) -> Result<Reply, ArchonError> {
    let feedback = ctx.store.lock().await.operator_feedback(id)?;
    let Some(curation) = &feedback.curation else {
        return Err(ArchonError::Invalid(format!(
            "feedback {id} asked for no memory to be forgotten"
        )));
    };
    match curation.state {
        CurationState::Pending | CurationState::Failed => {
            let feedback = forget(ctx, feedback).await?;
            receipt(ctx, feedback, false).await
        }
        CurationState::Applied => receipt(ctx, feedback, true).await,
        CurationState::Restored => Err(ArchonError::Invalid(format!(
            "memory {} was restored through {id}; forget it again in Aether's /memory",
            curation.memory
        ))),
    }
}

/// Restores the memory this correction forgot, unless something else
/// changed it since or restoring would not bring it back.
pub(super) async fn restore_memory(
    ctx: &DaemonContext,
    id: &FeedbackId,
) -> Result<Reply, ArchonError> {
    let feedback = ctx.store.lock().await.operator_feedback(id)?;
    let curation = restorable(&feedback)?.clone();
    let client = bound_client(ctx, &curation)?;
    let record = client
        .memory(&curation.memory)
        .await?
        .ok_or_else(|| missing(&client, &curation))?;
    still_ours(id, &curation, &record)?;
    let restored = client.restore_memory(&curation.memory).await?;
    assert!(restored.forgotten_at.is_none(), "checked by the adapter");
    let now = archon_core::now();
    let next = MemoryCuration {
        state: CurationState::Restored,
        restored_at: Some(now),
        updated_at: now,
        ..curation
    };
    let feedback = ctx.store.lock().await.update_curation(id, &next)?;
    receipt(ctx, feedback, false).await
}

fn restorable(feedback: &OperatorFeedback) -> Result<&MemoryCuration, ArchonError> {
    let id = &feedback.id;
    let Some(curation) = &feedback.curation else {
        return Err(ArchonError::Invalid(format!(
            "feedback {id} forgot no memory"
        )));
    };
    if curation.state != CurationState::Applied {
        return Err(ArchonError::Invalid(format!(
            "feedback {id} has not forgotten memory {}: its forget is {}",
            curation.memory,
            curation.state.as_str()
        )));
    }
    if curation.before != Some(MemoryBefore::Current) {
        return Err(ArchonError::Invalid(format!(
            "memory {} was already forgotten or superseded before {id}; restore it in \
             Aether's /memory if it should return",
            curation.memory
        )));
    }
    assert!(
        curation.forgotten_at.is_some(),
        "an applied forget is dated"
    );
    Ok(curation)
}

/// Refuses a restore that would undo someone else's curation or have no effect.
fn still_ours(
    id: &FeedbackId,
    curation: &MemoryCuration,
    record: &MemoryRecord,
) -> Result<(), ArchonError> {
    assert_eq!(record.id, curation.memory, "read by its id");
    let memory = &curation.memory;
    if let Some(newer) = &record.superseded_by {
        return Err(ArchonError::Invalid(format!(
            "memory {memory} is superseded by {newer}; restoring it would not bring it back"
        )));
    }
    match record.forgotten_at {
        None => Err(ArchonError::Invalid(format!(
            "memory {memory} is no longer forgotten: it was restored after {id} forgot it"
        ))),
        Some(at) if Some(at) != curation.forgotten_at => Err(ArchonError::Invalid(format!(
            "memory {memory} was forgotten again at {at}, after {id}; restore it in Aether's \
             /memory if it should return"
        ))),
        Some(_) => Ok(()),
    }
}

/// Forgets the correction's memory in its own Aether instance and records
/// Aether's answer; a failure is kept on the receipt, not returned.
async fn forget(
    ctx: &DaemonContext,
    feedback: OperatorFeedback,
) -> Result<OperatorFeedback, ArchonError> {
    let curation = feedback.curation.clone().expect("a forget was asked for");
    assert!(matches!(
        curation.state,
        CurationState::Pending | CurationState::Failed
    ));
    let reason = format!("archon {}: {}", feedback.id, feedback.reason);
    let mut next = curation;
    let result = match bound_client(ctx, &next) {
        Ok(client) => {
            next.instance_id = Some(client.welcome().instance_id.clone());
            forget_in(&client, &mut next, &reason).await
        }
        Err(error) => Err(error),
    };
    next.updated_at = archon_core::now();
    match result {
        Ok(()) => {
            next.state = CurationState::Applied;
            next.error = None;
        }
        Err(error) => {
            next.state = CurationState::Failed;
            next.error = Some(error.to_string());
            next.forgotten_at = None;
        }
    }
    ctx.store.lock().await.update_curation(&feedback.id, &next)
}

/// Reads the memory's state before the first forget, then forgets it. A
/// repeated forget converges: Aether answers a forgotten record unchanged.
async fn forget_in(
    client: &AetherClient,
    next: &mut MemoryCuration,
    reason: &str,
) -> Result<(), ArchonError> {
    if next.before.is_none() {
        let record = client
            .memory(&next.memory)
            .await?
            .ok_or_else(|| missing(client, next))?;
        next.before = Some(match (&record.superseded_by, record.forgotten_at) {
            (Some(_), _) => MemoryBefore::Superseded,
            (None, Some(_)) => MemoryBefore::Forgotten,
            (None, None) => MemoryBefore::Current,
        });
    }
    let record = client.forget_memory(&next.memory, reason).await?;
    assert_eq!(record.id, next.memory, "checked by the adapter");
    next.forgotten_at = record.forgotten_at;
    assert!(next.forgotten_at.is_some(), "checked by the adapter");
    Ok(())
}

fn missing(client: &AetherClient, curation: &MemoryCuration) -> ArchonError {
    ArchonError::NotFound(format!(
        "Aether instance {} has no memory {}",
        client.welcome().instance_id,
        curation.memory
    ))
}

/// The connected Aether, when it is the instance the forget is bound to and
/// speaks the memory commands.
fn bound_client(
    ctx: &DaemonContext,
    curation: &MemoryCuration,
) -> Result<Arc<AetherClient>, ArchonError> {
    let client = ctx.aether.client()?;
    let connected = &client.welcome().instance_id;
    if let Some(bound) = &curation.instance_id
        && bound != connected
    {
        return Err(ArchonError::Invalid(format!(
            "memory {} is curated in Aether instance {bound}, but archond is connected to \
             {connected}; nothing was sent",
            curation.memory
        )));
    }
    client.check_memory_protocol()?;
    Ok(client)
}

/// The instance a requested forget binds to: none while Aether is down. One
/// that cannot curate memories is refused before anything is recorded.
fn memory_instance(ctx: &DaemonContext) -> Result<Option<String>, ArchonError> {
    let client = match ctx.aether.client() {
        Ok(client) => client,
        Err(ArchonError::Unavailable(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    client.check_memory_protocol()?;
    let instance = client.welcome().instance_id.clone();
    assert!(!instance.is_empty(), "a welcome names its instance");
    Ok(Some(instance))
}

async fn correction_facts(
    ctx: &DaemonContext,
    request: &CorrectionRequest,
    aether_instance: Option<String>,
) -> Result<CorrectionFacts, ArchonError> {
    let (run, repo, delivery_commit) = {
        let store = ctx.store.lock().await;
        let run = store.run(&request.run)?;
        let repo = store.item(&run.item)?.repo;
        let commit = crate::executor::shipped_commit(&store, &run.id)?;
        (run, repo, commit)
    };
    let stage_kind = stage_kind(ctx, &run, repo.as_ref(), &request.stage).await?;
    Ok(CorrectionFacts {
        stage_kind,
        delivery_commit,
        aether_instance,
        observed_at: archon_core::now(),
    })
}

/// The stage's kind in the run's frozen pipeline, else the current catalog
/// once the finished run's snapshot is gone; `None` credits no model.
async fn stage_kind(
    ctx: &DaemonContext,
    run: &PipelineRun,
    repo: Option<&RepoId>,
    stage: &str,
) -> Result<Option<StageKind>, ArchonError> {
    let spec = match frozen::thaw(ctx, &run.id, &run.pipeline).await {
        Ok(spec) => Some(spec),
        Err(error) => {
            tracing::debug!(run = %run.id, %error, "run snapshot unavailable; using catalog");
            super::model_evidence::current_spec(ctx, repo, &run.pipeline).await?
        }
    };
    Ok(spec.and_then(|spec| spec.stage(stage).map(|s| s.kind)))
}

async fn receipt(
    ctx: &DaemonContext,
    feedback: OperatorFeedback,
    replayed: bool,
) -> Result<Reply, ArchonError> {
    let view = view(ctx, feedback).await?;
    Ok(Reply::Feedback(Box::new(FeedbackReceipt {
        view,
        replayed,
    })))
}

/// What a correction does now: the standing of its choice and of the
/// cohort's default, the item's frozen explanation, and the memory's state.
async fn view(
    ctx: &DaemonContext,
    feedback: OperatorFeedback,
) -> Result<FeedbackView, ArchonError> {
    let (memory, memory_error) = match &feedback.curation {
        Some(curation) => memory_state(ctx, curation).await,
        None => (None, None),
    };
    let attribution = &feedback.attribution;
    let policy = cohort_policy(ctx, attribution.repo.as_ref()).await?;
    let as_of = archon_core::now();
    let query = RouteQuery {
        repo: attribution.repo.as_ref(),
        pipeline: &attribution.pipeline,
        stage: &feedback.stage,
        size: None,
        context: attribution.context.as_ref(),
    };
    let corrected = feedback.penalized().map(|(_, choice)| choice);
    let (corrected, selected, frozen_explanation) = {
        let store = ctx.store.lock().await;
        let (corrected, selected) =
            crate::learned::standings(&store, &query, &policy, corrected, as_of)?;
        let enabled = store.learning_config()?.enabled;
        let frozen = frozen_explanation(&store, &feedback)?;
        (corrected, selected.filter(|_| enabled), frozen)
    };
    Ok(FeedbackView {
        penalizing: feedback.penalizing(as_of),
        feedback,
        corrected,
        selected,
        frozen_explanation,
        memory,
        memory_error,
    })
}

/// The model policy of the correction's repository; the global one once
/// that repository is no longer registered.
async fn cohort_policy(
    ctx: &DaemonContext,
    repo: Option<&RepoId>,
) -> Result<ModelPolicy, ArchonError> {
    match RepoEnv::load(ctx, repo).await {
        Ok(env) => Ok(env.policy(&ctx.config)),
        Err(ArchonError::NotFound(_)) => Ok(ctx.config.models.policy()),
        Err(error) => Err(error),
    }
}

fn frozen_explanation(
    store: &archon_queue::Store,
    feedback: &OperatorFeedback,
) -> Result<Option<String>, ArchonError> {
    let key = format!("learning/routing/{}", feedback.item);
    let Some(raw) = store.kv_get(&key)? else {
        return Ok(None);
    };
    let mut reasons: BTreeMap<String, String> =
        serde_json::from_str(&raw).map_err(|e| ArchonError::Store(format!("kv {key}: {e}")))?;
    Ok(reasons.remove(&feedback.stage))
}

/// The memory's current record, or why it could not be read: the reply
/// still shows the recorded correction.
async fn memory_state(
    ctx: &DaemonContext,
    curation: &MemoryCuration,
) -> (Option<MemoryView>, Option<String>) {
    let read = async {
        let client = bound_client(ctx, curation)?;
        client
            .memory(&curation.memory)
            .await?
            .ok_or_else(|| missing(&client, curation))
    };
    match read.await {
        Ok(record) => (Some(record.view()), None),
        Err(error) => (None, Some(error.to_string())),
    }
}
