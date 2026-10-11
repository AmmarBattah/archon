//! Autonomy policy and operator directives (ARCHITECTURE §18.2, §18.5).

use archon_core::autonomy::{AutonomyView, Directive, DirectiveSource};
use archon_core::error::ArchonError;
use archon_core::id::DirectiveId;
use archon_core::protocol::{ClientKind, Reply, RepoRef};

use crate::context::DaemonContext;
use crate::feed;
use crate::resolve::RepoEnv;

/// Defaults, ceiling and directives for one repository, or globally.
pub(super) async fn view(
    ctx: &DaemonContext,
    repo: Option<&RepoRef>,
) -> Result<Reply, ArchonError> {
    let repo = match repo {
        Some(reference) => Some(ctx.store.lock().await.repo(reference)?),
        None => None,
    };
    let env = RepoEnv::new(repo)?;
    let policy = env.autonomy(&ctx.config);
    let directives = ctx.store.lock().await.directives(archon_core::now())?;
    assert!(
        policy.default.excess(policy.ceiling).is_empty(),
        "defaults sit within the ceiling"
    );
    Ok(Reply::Autonomy(Box::new(AutonomyView {
        repo: env.repo_name().map(str::to_owned),
        policy,
        directives,
    })))
}

/// The orchestrator records what the operator said in chat; the CLI is
/// the operator directly. Either way the feed shows it.
pub(super) async fn set_directive(
    ctx: &DaemonContext,
    client: ClientKind,
    text: &str,
    expires_in_hours: Option<u32>,
) -> Result<Reply, ArchonError> {
    let source = match client {
        ClientKind::ArchonMcp => DirectiveSource::Orchestrator,
        ClientKind::ArchonCli | ClientKind::ArchonBench => DirectiveSource::Operator,
    };
    let directive = Directive::new(text, source, expires_in_hours, archon_core::now())?;
    ctx.store.lock().await.insert_directive(&directive)?;
    let until = match directive.expires_at {
        Some(at) => format!(" until {}", at.format("%Y-%m-%d %H:%M UTC")),
        None => String::new(),
    };
    let note = format!(
        "Directive {} in force{until}: {}",
        directive.id, directive.text
    );
    feed::system(ctx, note).await?;
    Ok(Reply::Directive(directive))
}

pub(super) async fn clear_directive(
    ctx: &DaemonContext,
    directive: Option<&DirectiveId>,
) -> Result<Reply, ArchonError> {
    let cleared = ctx.store.lock().await.clear_directives(directive)?;
    let note = match directive {
        Some(id) => format!("Directive {id} cleared."),
        None => format!("All directives cleared ({cleared})."),
    };
    if cleared > 0 {
        feed::system(ctx, note).await?;
    }
    Ok(Reply::Cleared(cleared))
}
