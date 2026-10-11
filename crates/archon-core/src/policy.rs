//! Which models the orchestrator may assign. The orchestrator chooses; this
//! decides whether the choice is allowed. Enforced by the daemon at enqueue,
//! at spawn and by the fleet watchdog (ARCHITECTURE §7.4).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::ArchonError;
use crate::model::{Effort, ModelChoice, ModelPlan, ModelRef};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPolicy {
    /// `None` allows any model; `Some(empty)` allows none (an empty
    /// intersection of global and repository allowlists).
    pub allow: Option<BTreeSet<ModelRef>>,
    pub deny: BTreeSet<ModelRef>,
    pub max_effort: Option<Effort>,
}

impl ModelPolicy {
    /// Build from config lists, where an empty `allow` means "any model".
    pub fn from_lists(allow: &[ModelRef], deny: &[ModelRef], max_effort: Option<Effort>) -> Self {
        let allow = (!allow.is_empty()).then(|| allow.iter().cloned().collect());
        Self {
            allow,
            deny: deny.iter().cloned().collect(),
            max_effort,
        }
    }

    /// A repository may only narrow: its allowlist is intersected with this
    /// one, its deny list is added, and the lower effort cap wins.
    pub fn narrowed_by(&self, repo: &ModelPolicy) -> ModelPolicy {
        let allow = match (&self.allow, &repo.allow) {
            (None, other) | (other, None) => other.clone(),
            (Some(global), Some(local)) => Some(global.intersection(local).cloned().collect()),
        };
        let max_effort = match (self.max_effort, repo.max_effort) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let narrowed = ModelPolicy {
            allow,
            deny: self.deny.union(&repo.deny).cloned().collect(),
            max_effort,
        };
        debug_assert!(self.deny.is_subset(&narrowed.deny));
        narrowed
    }

    pub fn permits_model(&self, model: &ModelRef) -> bool {
        let allowed = self.allow.as_ref().is_none_or(|set| set.contains(model));
        allowed && !self.deny.contains(model)
    }

    /// Refuse a choice outside the policy with an error that names what is
    /// allowed, so the orchestrator can correct itself.
    pub fn check(&self, choice: &ModelChoice) -> Result<(), ArchonError> {
        if !self.permits_model(&choice.model) {
            return Err(ArchonError::Policy(format!(
                "model {} is not allowed; allowed: {}",
                choice.model,
                self.describe_allowed()
            )));
        }
        if let Some(cap) = self.max_effort
            && choice.effort > cap
        {
            return Err(ArchonError::Policy(format!(
                "effort {} exceeds the configured maximum {cap}",
                choice.effort
            )));
        }
        Ok(())
    }

    pub fn check_plan(&self, plan: &ModelPlan) -> Result<(), ArchonError> {
        for (stage, choices) in &plan.0 {
            if choices.is_empty() {
                return Err(ArchonError::Invalid(format!(
                    "model_plan stage {stage:?} lists no model"
                )));
            }
            for choice in choices {
                self.check(choice)
                    .map_err(|e| ArchonError::Policy(format!("stage {stage:?}: {e}")))?;
            }
        }
        Ok(())
    }

    pub fn describe_allowed(&self) -> String {
        let listed = match &self.allow {
            None => return "any model Aether lists".to_owned(),
            Some(set) if set.is_empty() => return "none (allowlists do not overlap)".to_owned(),
            Some(set) => set,
        };
        listed
            .iter()
            .filter(|m| !self.deny.contains(m))
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> ModelRef {
        s.parse().unwrap()
    }

    fn choice(model: &str, effort: Effort) -> ModelChoice {
        ModelChoice {
            model: m(model),
            effort,
        }
    }

    #[test]
    fn empty_allowlist_permits_anything_not_denied() {
        let policy = ModelPolicy::from_lists(&[], &[m("codex/x")], None);
        assert!(
            policy
                .check(&choice("claude-code/opus", Effort::Max))
                .is_ok()
        );
        assert!(policy.check(&choice("codex/x", Effort::Low)).is_err());
    }

    #[test]
    fn allowlist_errors_name_the_allowed_models() {
        let policy = ModelPolicy::from_lists(&[m("claude-code/claude-opus-5-5")], &[], None);
        let err = policy
            .check(&choice("codex/x", Effort::High))
            .unwrap_err()
            .to_string();
        assert!(err.contains("claude-code/claude-opus-5-5"), "{err}");
    }

    #[test]
    fn repository_policy_only_narrows() {
        let global = ModelPolicy::from_lists(&[m("a/1"), m("b/2")], &[], Some(Effort::Max));
        let repo = ModelPolicy::from_lists(&[m("b/2"), m("c/3")], &[], Some(Effort::High));
        let eff = global.narrowed_by(&repo);
        assert!(eff.permits_model(&m("b/2")));
        assert!(!eff.permits_model(&m("a/1")));
        assert!(!eff.permits_model(&m("c/3")));
        assert_eq!(eff.max_effort, Some(Effort::High));

        let disjoint = ModelPolicy::from_lists(&[m("z/9")], &[], None);
        assert!(!global.narrowed_by(&disjoint).permits_model(&m("a/1")));
    }

    #[test]
    fn effort_cap_is_enforced() {
        let policy = ModelPolicy::from_lists(&[], &[], Some(Effort::High));
        assert!(policy.check(&choice("a/1", Effort::High)).is_ok());
        assert!(policy.check(&choice("a/1", Effort::Max)).is_err());
    }
}
