//! Operator corrections (`archon learning`): bounded, reversible records
//! beside the immutable outcomes. Nothing here touches the legacy
//! corrections list or lesson uses.

mod attribution;

use archon_core::Timestamp;
use archon_core::catalog::StageKind;
use archon_core::error::ArchonError;
use archon_core::feedback::{
    CorrectionRequest, Credit, MAX_FEEDBACK_PER_RUN, MemoryCuration, OperatorFeedback,
};
use archon_core::id::{FeedbackId, QueueItemId, RepoId};
use archon_core::outcome::RouteContext;
use rusqlite::{Connection, OptionalExtension, params};

use super::Store;
use super::codec::{SqlContext, from_json, to_json, ts};

/// What the daemon knows about a correction that the store does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionFacts {
    /// The corrected stage's kind in the run's frozen pipeline, when known.
    pub stage_kind: Option<StageKind>,
    pub delivery_commit: Option<String>,
    /// The connected Aether instance, which a requested forget is bound to.
    pub aether_instance: Option<String>,
    pub observed_at: Timestamp,
}

impl Store {
    /// Records a correction, or returns the unrevoked one recorded for the
    /// same request with `true`. Refuses ids that do not belong together.
    pub fn record_operator_correction(
        &self,
        request: &CorrectionRequest,
        facts: &CorrectionFacts,
    ) -> Result<(OperatorFeedback, bool), ArchonError> {
        request.check()?;
        if !self.learning_config()?.enabled {
            return Err(ArchonError::Invalid(
                "learning is disabled, so no correction is recorded; enable [learning] first"
                    .into(),
            ));
        }
        let feedback = attribution::feedback(self, request, facts)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .ctx("begin operator correction")?;
        if let Some(existing) = by_request(&tx, &feedback.request_key)? {
            assert_eq!(existing.run, feedback.run, "a request key names its run");
            return Ok((existing, true));
        }
        let recorded: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM operator_feedback WHERE run = ?1",
                [feedback.run.as_str()],
                |row| row.get(0),
            )
            .ctx("count run corrections")?;
        if recorded >= MAX_FEEDBACK_PER_RUN as i64 {
            return Err(ArchonError::Invalid(format!(
                "run {} already has {MAX_FEEDBACK_PER_RUN} corrections",
                feedback.run
            )));
        }
        insert(&tx, &feedback)?;
        tx.commit().ctx("commit operator correction")?;
        Ok((feedback, false))
    }

    pub fn operator_feedback(&self, id: &FeedbackId) -> Result<OperatorFeedback, ArchonError> {
        load(&self.conn, id)?.ok_or_else(|| ArchonError::NotFound(format!("feedback {id}")))
    }

    /// Every correction of the item's runs, oldest first.
    pub fn item_feedback(&self, item: &QueueItemId) -> Result<Vec<OperatorFeedback>, ArchonError> {
        let mut statement = self
            .conn
            .prepare("SELECT data FROM operator_feedback WHERE item = ?1 ORDER BY observed_at, id")
            .ctx("prepare item corrections")?;
        let rows = statement
            .query_map([item.as_str()], |row| row.get::<_, String>(0))
            .ctx("query item corrections")?;
        let mut feedback = Vec::new();
        for raw in rows {
            let event: OperatorFeedback = from_json(&raw.ctx("read item correction")?, "feedback")?;
            assert_eq!(&event.item, item, "a correction is read by its item");
            feedback.push(event);
        }
        Ok(feedback)
    }

    /// Revokes the correction's model penalty at `at`; a revoked one stays as it was.
    pub fn revoke_feedback(
        &self,
        id: &FeedbackId,
        at: Timestamp,
    ) -> Result<OperatorFeedback, ArchonError> {
        let tx = self
            .conn
            .unchecked_transaction()
            .ctx("begin correction undo")?;
        let mut feedback =
            load(&tx, id)?.ok_or_else(|| ArchonError::NotFound(format!("feedback {id}")))?;
        if feedback.revoked_at.is_some() {
            return Ok(feedback);
        }
        feedback.revoked_at = Some(at.max(feedback.observed_at));
        feedback.check()?;
        update(&tx, &feedback)?;
        tx.commit().ctx("commit correction undo")?;
        Ok(feedback)
    }

    /// Replaces the correction's memory curation, which must name the same memory.
    pub fn update_curation(
        &self,
        id: &FeedbackId,
        curation: &MemoryCuration,
    ) -> Result<OperatorFeedback, ArchonError> {
        curation.check()?;
        let tx = self
            .conn
            .unchecked_transaction()
            .ctx("begin memory curation")?;
        let mut feedback =
            load(&tx, id)?.ok_or_else(|| ArchonError::NotFound(format!("feedback {id}")))?;
        let Some(current) = &feedback.curation else {
            return Err(ArchonError::Invalid(format!(
                "feedback {id} asked for no memory curation"
            )));
        };
        let rebound = current
            .instance_id
            .as_ref()
            .is_some_and(|bound| curation.instance_id.as_ref() != Some(bound));
        if current.memory != curation.memory || rebound {
            return Err(ArchonError::Invalid(format!(
                "feedback {id} curates {} in its own Aether instance only",
                current.memory
            )));
        }
        feedback.curation = Some(curation.clone());
        feedback.check()?;
        update(&tx, &feedback)?;
        tx.commit().ctx("commit memory curation")?;
        Ok(feedback)
    }
}

/// The quality corrections credited to a model in one repository's cohort
/// of `stage`; `None` context matches every context.
pub(super) fn cohort_corrections(
    conn: &Connection,
    repo: &RepoId,
    pipeline: &str,
    stage: &str,
    context: Option<&RouteContext>,
    as_of: Timestamp,
) -> Result<Vec<OperatorFeedback>, ArchonError> {
    let mut statement = conn
        .prepare(
            "SELECT data FROM operator_feedback WHERE repo = ?1 AND pipeline = ?2 AND stage = ?3
             AND stage_run IS NOT NULL AND cause = 'quality' AND observed_at <= ?4
             AND (?5 IS NULL OR (json_extract(data, '$.attribution.context.revision') = ?5
                 AND json_extract(data, '$.attribution.context.review') = ?6
                 AND json_extract(data, '$.attribution.context.task_class') = ?7))
             ORDER BY observed_at, id",
        )
        .ctx("prepare cohort corrections")?;
    let rows = statement
        .query_map(
            params![
                repo.as_str(),
                pipeline,
                stage,
                ts(&as_of),
                context.map(|c| &c.revision),
                context.map(|c| &c.review),
                context.map(|c| &c.task_class),
            ],
            |row| row.get::<_, String>(0),
        )
        .ctx("query cohort corrections")?;
    let mut feedback = Vec::new();
    for raw in rows {
        let event: OperatorFeedback = from_json(&raw.ctx("read cohort correction")?, "feedback")?;
        assert!(
            event.penalized().is_some(),
            "only credited quality corrections are read"
        );
        assert!(context.is_none_or(|c| event.attribution.context.as_ref() == Some(c)));
        feedback.push(event);
    }
    Ok(feedback)
}

fn by_request(conn: &Connection, key: &str) -> Result<Option<OperatorFeedback>, ArchonError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT data FROM operator_feedback WHERE request_key = ?1 AND revoked_at IS NULL",
            [key],
            |row| row.get(0),
        )
        .optional()
        .ctx("read correction by request")?;
    raw.map(|raw| from_json(&raw, "feedback")).transpose()
}

fn load(conn: &Connection, id: &FeedbackId) -> Result<Option<OperatorFeedback>, ArchonError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT data FROM operator_feedback WHERE id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .ctx("read correction")?;
    raw.map(|raw| from_json(&raw, "feedback")).transpose()
}

fn insert(conn: &Connection, feedback: &OperatorFeedback) -> Result<(), ArchonError> {
    feedback.check()?;
    let choice = match &feedback.attribution.credit {
        Credit::Model { stage_run, choice } => Some((
            stage_run.as_str(),
            choice.model.to_string(),
            choice.effort.to_string(),
        )),
        Credit::Unavailable { .. } => None,
    };
    assert!(
        feedback.penalized().is_none() || choice.is_some(),
        "a penalty names its choice"
    );
    conn.execute(
        "INSERT INTO operator_feedback (id, request_key, item, run, stage, stage_run, model,
             effort, repo, pipeline, cause, reason, observed_at, revoked_at, aether_instance_id,
             memory_id, curation, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            feedback.id.as_str(),
            feedback.request_key,
            feedback.item.as_str(),
            feedback.run.as_str(),
            feedback.stage,
            choice.as_ref().map(|c| c.0),
            choice.as_ref().map(|c| &c.1),
            choice.as_ref().map(|c| &c.2),
            feedback.attribution.repo.as_ref().map(RepoId::as_str),
            feedback.attribution.pipeline,
            feedback.cause.as_str(),
            feedback.reason,
            ts(&feedback.observed_at),
            feedback.revoked_at.as_ref().map(ts),
            curation_instance(feedback),
            feedback.curation.as_ref().map(|c| c.memory.as_str()),
            feedback.curation.as_ref().map(|c| c.state.as_str()),
            to_json(feedback, "feedback")?,
        ],
    )
    .ctx("insert operator correction")?;
    Ok(())
}

/// Writes the parts that change: revocation and curation.
fn update(conn: &Connection, feedback: &OperatorFeedback) -> Result<(), ArchonError> {
    let changed = conn
        .execute(
            "UPDATE operator_feedback SET revoked_at = ?2, aether_instance_id = ?3, curation = ?4,
                 data = ?5
             WHERE id = ?1",
            params![
                feedback.id.as_str(),
                feedback.revoked_at.as_ref().map(ts),
                curation_instance(feedback),
                feedback.curation.as_ref().map(|c| c.state.as_str()),
                to_json(feedback, "feedback")?,
            ],
        )
        .ctx("update operator correction")?;
    assert_eq!(
        changed, 1,
        "an update follows a load of the same correction"
    );
    Ok(())
}

fn curation_instance(feedback: &OperatorFeedback) -> Option<&str> {
    feedback.curation.as_ref()?.instance_id.as_deref()
}
