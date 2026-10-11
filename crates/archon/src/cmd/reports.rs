//! `archon reports [reopen <id>]`: published reports, and showing a closed
//! one again on demand.

use std::process::ExitCode;

use archon_core::id::ReportId;
use archon_core::protocol::{Reply, Request};

use super::{emit, to_json};
use crate::cli::ReportCommand;
use crate::connect::{connect, unexpected};
use crate::format;

pub async fn run(command: Option<ReportCommand>, json: bool) -> anyhow::Result<ExitCode> {
    match command {
        None => list(json).await?,
        Some(ReportCommand::Reopen { report }) => reopen(&report, json).await?,
    }
    Ok(ExitCode::SUCCESS)
}

async fn list(json: bool) -> anyhow::Result<()> {
    let client = connect().await?;
    let reports = match client.request(Request::ListReports).await? {
        Reply::Reports(reports) => reports,
        other => return Err(unexpected(&other, "reports")),
    };
    emit(json, || to_json(&reports), &format::reports(&reports))
}

async fn reopen(report: &str, json: bool) -> anyhow::Result<()> {
    let report = ReportId::parse(report.trim()).map_err(|e| {
        archon_core::ArchonError::Invalid(format!("{report:?} is not a report id (rpt_…): {e}"))
    })?;
    let client = connect().await?;
    let published = match client.request(Request::ReopenReport { report }).await? {
        Reply::Report(published) => published,
        other => return Err(unexpected(&other, "report")),
    };
    emit(json, || to_json(&published), &format::reopened(&published))
}
