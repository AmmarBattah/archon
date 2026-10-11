//! The spend ledger. `agent.usage_updated` carries absolute totals, so the
//! `spend` table keeps one row per agent with what has been charged for it.
//! `spend_daily` splits each agent's charge into the local calendar days in
//! which it grew, which is what the daily budget needs: an agent that spent
//! $40 yesterday and $1 today counts $1 today.
//!
//! A charge never goes down. A lower total (a usage correction) keeps the
//! amounts already charged, cost and estimate separately, so no day gets a
//! credit that would cancel other agents' spend under the daily budget.
//! Each growth amount retains its observed basis, even if later totals have
//! different provenance. Mixed totals provide no split for their growth.

use archon_core::Timestamp;
use archon_core::aether_ids::AetherAgentId;
use archon_core::cost::CostBasis;
use archon_core::error::ArchonError;
use archon_core::id::{PipelineRunId, QueueItemId};
use rusqlite::{Connection, OptionalExtension, params};

use super::SpendRow;
use super::codec::{SqlContext, to_i64, ts, write_tx};

/// The local calendar day of `at`, as `YYYY-MM-DD`.
pub(super) fn local_day(at: &Timestamp) -> String {
    at.with_timezone(&chrono::Local)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn check_row(row: &SpendRow) -> Result<(), ArchonError> {
    for (name, value) in [
        ("cost_usd", row.cost_usd),
        ("estimated_cost_usd", row.estimated_cost_usd),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(ArchonError::Invalid(format!(
                "{name} {value} for agent {} is not a non-negative amount",
                row.agent
            )));
        }
    }
    Ok(())
}

/// Raw charged amounts, event time and provenance of the agent's current row.
fn current(
    conn: &Connection,
    row: &SpendRow,
) -> Result<Option<(f64, f64, String, String)>, ArchonError> {
    conn.query_row(
        "SELECT cost_usd, estimated_cost_usd, at, cost_basis FROM spend WHERE agent = ?1",
        [row.agent.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()
    .ctx("read spend row")
}

fn upsert_latest(conn: &Connection, row: &SpendRow) -> Result<(), ArchonError> {
    conn.execute(
        "INSERT INTO spend (agent, item, cost_usd, estimated_cost_usd, input_tokens,
                            output_tokens, at, cost_basis)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (agent) DO UPDATE SET item = excluded.item,
             cost_usd = excluded.cost_usd, estimated_cost_usd = excluded.estimated_cost_usd,
             input_tokens = excluded.input_tokens, output_tokens = excluded.output_tokens,
             at = excluded.at, cost_basis = excluded.cost_basis",
        params![
            row.agent.as_str(),
            row.item.as_ref().map(QueueItemId::as_str),
            row.cost_usd,
            row.estimated_cost_usd,
            to_i64(row.input_tokens, "input tokens")?,
            to_i64(row.output_tokens, "output tokens")?,
            ts(&row.at),
            row.cost_basis.name(),
        ],
    )
    .ctx("write spend row")?;
    Ok(())
}

fn add_to_day(
    conn: &Connection,
    row: &SpendRow,
    cost: f64,
    estimate: f64,
) -> Result<(), ArchonError> {
    let (billed, list_price, unclassified) = match row.cost_basis {
        CostBasis::ProviderReported => (cost, 0.0, 0.0),
        CostBasis::ListPrice => (0.0, cost, 0.0),
        CostBasis::Unknown | CostBasis::Mixed => (0.0, 0.0, cost),
    };
    conn.execute(
        "INSERT INTO spend_daily (agent, day, cost_usd, estimated_cost_usd,
                                  billed_cost_usd, list_price_cost_usd, unclassified_cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (agent, day) DO UPDATE SET
             cost_usd = cost_usd + excluded.cost_usd,
             estimated_cost_usd = estimated_cost_usd + excluded.estimated_cost_usd,
             billed_cost_usd = billed_cost_usd + excluded.billed_cost_usd,
             list_price_cost_usd = list_price_cost_usd + excluded.list_price_cost_usd,
             unclassified_cost_usd = unclassified_cost_usd + excluded.unclassified_cost_usd",
        params![
            row.agent.as_str(),
            local_day(&row.at),
            cost,
            estimate,
            billed,
            list_price,
            unclassified
        ],
    )
    .ctx("write daily spend")?;
    Ok(())
}

/// `row` with each amount raised to what `charged` already holds, and the
/// growth `(cost, estimate)` that still needs charging.
fn clamp_to_charged(row: &SpendRow, charged: (f64, f64)) -> (SpendRow, f64, f64) {
    let (old_cost, old_estimate) = charged;
    let kept = SpendRow {
        cost_usd: row.cost_usd.max(old_cost),
        estimated_cost_usd: row.estimated_cost_usd.max(old_estimate),
        ..row.clone()
    };
    if kept != *row {
        tracing::warn!(
            agent = %row.agent,
            cost = row.cost_usd,
            estimate = row.estimated_cost_usd,
            "usage total went down; keeping what was already charged"
        );
    }
    let growth = (
        kept.cost_usd - old_cost,
        kept.estimated_cost_usd - old_estimate,
    );
    debug_assert!(growth.0 >= 0.0 && growth.1 >= 0.0, "a charge never shrinks");
    (kept, growth.0, growth.1)
}

/// Charges the agent's growth since its last row. A row older than the
/// stored one is a replayed event and changes nothing; a replay of the same
/// row adds nothing, and a lower total keeps what was already charged.
pub(super) fn record(conn: &mut Connection, row: &SpendRow) -> Result<(), ArchonError> {
    check_row(row)?;
    let tx = write_tx(conn)?;
    if let Some(item) = &row.item {
        super::items::load(&tx, item)?;
    }
    let (old_cost, old_estimate, old_basis) = match current(&tx, row)? {
        Some((_, _, at, _)) if at > ts(&row.at) => {
            tracing::debug!(agent = %row.agent, "ignoring an older usage total");
            return Ok(());
        }
        Some((cost, estimate, _, basis)) => (cost, estimate, basis),
        None => (0.0, 0.0, row.cost_basis.name().to_owned()),
    };
    let (mut kept, cost, estimate) = clamp_to_charged(row, (old_cost, old_estimate));
    if row.cost_usd < old_cost && row.cost_basis.name() != old_basis {
        // The new provenance describes a smaller amount, not the charge retained
        // by the budget. Its entire split can no longer be established.
        kept.cost_basis = archon_core::cost::CostBasis::Unknown;
    }
    upsert_latest(&tx, &kept)?;
    add_to_day(&tx, &kept, cost, estimate)?;
    tx.commit().ctx("commit spend")
}

pub(super) fn for_item(
    conn: &Connection,
    item: &QueueItemId,
    estimates: bool,
) -> Result<f64, ArchonError> {
    super::items::load(conn, item)?;
    let total: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(cost_usd + CASE WHEN ?2 THEN estimated_cost_usd ELSE 0 END), 0)
             FROM spend WHERE item = ?1",
            params![item.as_str(), estimates],
            |r| r.get(0),
        )
        .ctx("sum item spend")?;
    debug_assert!(total.is_finite());
    Ok(total)
}

/// What the agent has been charged; 0 before its first usage event.
pub(super) fn for_agent(
    conn: &Connection,
    agent: &AetherAgentId,
    estimates: bool,
) -> Result<f64, ArchonError> {
    let total: Option<f64> = conn
        .query_row(
            "SELECT cost_usd + CASE WHEN ?2 THEN estimated_cost_usd ELSE 0 END
             FROM spend WHERE agent = ?1",
            params![agent.as_str(), estimates],
            |r| r.get(0),
        )
        .optional()
        .ctx("read agent spend")?;
    let total = total.unwrap_or(0.0);
    debug_assert!(
        total.is_finite() && total >= 0.0,
        "rows are checked on write"
    );
    Ok(total)
}

pub(super) fn totals(
    conn: &Connection,
    now: &Timestamp,
    estimates: bool,
) -> Result<(f64, f64), ArchonError> {
    let today: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(cost_usd + CASE WHEN ?2 THEN estimated_cost_usd ELSE 0 END), 0)
             FROM spend_daily WHERE day = ?1",
            params![local_day(now), estimates],
            |r| r.get(0),
        )
        .ctx("sum daily spend")?;
    let total: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(cost_usd + CASE WHEN ?1 THEN estimated_cost_usd ELSE 0 END), 0)
             FROM spend",
            [estimates],
            |r| r.get(0),
        )
        .ctx("sum total spend")?;
    debug_assert!(today.is_finite() && total.is_finite());
    Ok((today, total))
}

/// Bind before receiving usage, so descendants and late events retain ownership.
pub(super) fn link_run(
    conn: &Connection,
    agent: &AetherAgentId,
    run: &PipelineRunId,
) -> Result<(), ArchonError> {
    super::runs::load(conn, run)?;
    if let Some(owned) = agent_run(conn, agent)? {
        if owned != *run {
            return Err(ArchonError::Invalid(format!(
                "agent {agent} already belongs to run {owned}"
            )));
        }
        return Ok(());
    }
    conn.execute(
        "INSERT INTO agent_runs (agent, run) VALUES (?1, ?2)",
        params![agent.as_str(), run.as_str()],
    )
    .ctx("link agent to run")?;
    Ok(())
}

pub(super) fn agent_run(
    conn: &Connection,
    agent: &AetherAgentId,
) -> Result<Option<PipelineRunId>, ArchonError> {
    let (stage, linked): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT st.run, a.run FROM (SELECT ?1 AS agent) input
         LEFT JOIN stage_runs st ON st.aether_agent = input.agent
         LEFT JOIN agent_runs a ON a.agent = input.agent",
            [agent.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ctx("read agent run")?;
    if stage
        .as_ref()
        .zip(linked.as_ref())
        .is_some_and(|(a, b)| a != b)
    {
        return Err(ArchonError::Store(format!(
            "agent {agent} has inconsistent run ownership"
        )));
    }
    let raw = stage.or(linked);
    raw.map(|raw| {
        PipelineRunId::parse(&raw)
            .map_err(|e| ArchonError::Store(format!("agent {agent} run: {e}")))
    })
    .transpose()
}

pub(super) fn for_run(
    conn: &Connection,
    run: &PipelineRunId,
    estimates: bool,
) -> Result<f64, ArchonError> {
    super::runs::load(conn, run)?;
    let total = conn.query_row(
        "SELECT COALESCE(SUM(s.cost_usd + CASE WHEN ?2 THEN s.estimated_cost_usd ELSE 0 END), 0)
         FROM spend s WHERE s.agent IN (
             SELECT aether_agent FROM stage_runs WHERE run = ?1 AND aether_agent IS NOT NULL
             UNION SELECT agent FROM agent_runs WHERE run = ?1
         )",
        params![run.as_str(), estimates], |r| r.get(0)).ctx("sum run spend")?;
    Ok(total)
}
