//! `CostReport`: the spend ledger summed as the budgets count it.

use archon_core::cost::CostQuery;
use archon_core::error::ArchonError;
use archon_core::protocol::Reply;

use crate::context::DaemonContext;

/// Relative bounds count back from today in archond's local time zone, the
/// one the ledger's days and the daily budget use.
pub(super) async fn report(ctx: &DaemonContext, query: &CostQuery) -> Result<Reply, ArchonError> {
    let today = archon_core::now()
        .with_timezone(&chrono::Local)
        .date_naive();
    let range = query.range(today)?;
    let estimates = ctx.config.limits.count_estimated_cost;
    let report = ctx
        .store
        .lock()
        .await
        .cost_report(&range, query.by, estimates)?;
    assert_eq!(report.range, range, "the report covers the days asked for");
    assert_eq!(report.estimates_counted, estimates);
    Ok(Reply::CostReport(Box::new(report)))
}
