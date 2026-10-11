//! What applies to one repository: its `.archon/config.toml`, the model
//! policy narrowed by it, its autonomy policy, its pipeline catalog, and
//! the daemon's caps.

use archon_core::autonomy::AutonomyPolicy;
use archon_core::config::{Config, RepoConfig};
use archon_core::error::ArchonError;
use archon_core::id::RepoId;
use archon_core::policy::ModelPolicy;
use archon_core::protocol::RepoRef;
use archon_core::queue::Caps;
use archon_core::repo::Repo;
use archon_pipeline::{Catalog, CatalogSources};

use crate::context::DaemonContext;

/// The caps in force: the `SetCaps` override, else `[limits]`.
pub(crate) async fn caps(ctx: &DaemonContext) -> Caps {
    match &*ctx.caps_override.lock().await {
        Some(caps) => caps.clone(),
        None => ctx.config.limits.caps(),
    }
}

pub(crate) fn repo_ref(id: &RepoId) -> RepoRef {
    RepoRef(id.as_str().to_owned())
}

/// A repository (or none, for repo-less items) with its local config.
#[derive(Debug, Clone)]
pub(crate) struct RepoEnv {
    pub repo: Option<Repo>,
    pub config: RepoConfig,
}

impl RepoEnv {
    /// Reads `<repo>/.archon/config.toml`; a broken file is an error so a
    /// narrowed model policy can never be skipped silently.
    pub fn new(repo: Option<Repo>) -> Result<Self, ArchonError> {
        let config = match &repo {
            Some(repo) => RepoConfig::load(&repo.path)?,
            None => RepoConfig::default(),
        };
        Ok(Self { repo, config })
    }

    /// Looks up `repo` (when given) in the store, then reads its config.
    /// The store lock is only held for the lookup.
    pub async fn load(ctx: &DaemonContext, repo: Option<&RepoId>) -> Result<Self, ArchonError> {
        let repo = match repo {
            Some(id) => Some(ctx.store.lock().await.repo(&repo_ref(id))?),
            None => None,
        };
        Self::new(repo)
    }

    /// The global policy narrowed by the repository's `[models]`.
    pub fn policy(&self, config: &Config) -> ModelPolicy {
        config
            .models
            .policy()
            .narrowed_by(&self.config.models.policy())
    }

    /// Global `[autonomy]`, the registry ceiling, then the repository's own
    /// file, which may only lower (ARCHITECTURE §18.2).
    pub fn autonomy(&self, config: &Config) -> AutonomyPolicy {
        let registry = self.repo.as_ref().map(|r| &r.autonomy_max);
        let file = self.repo.as_ref().map(|_| &self.config.autonomy);
        AutonomyPolicy::resolve(&config.autonomy, registry, file)
    }

    /// How ceiling refusals name where the ceiling applies.
    pub fn scope(&self) -> String {
        match self.repo_name() {
            Some(name) => format!("repository {name:?}"),
            None => "items without a repository".to_owned(),
        }
    }

    /// Built-in, user and repository pipelines, minus the disabled ones.
    pub fn catalog(&self) -> Result<Catalog, ArchonError> {
        Catalog::discover(&CatalogSources {
            user_dir: Some(archon_core::paths::config_dir().join("pipelines")),
            repo_root: self.repo.as_ref().map(|r| r.path.clone()),
            disabled: self.config.pipelines.disabled.clone(),
        })
    }

    pub fn repo_name(&self) -> Option<&str> {
        self.repo.as_ref().map(|r| r.name.as_str())
    }
}
