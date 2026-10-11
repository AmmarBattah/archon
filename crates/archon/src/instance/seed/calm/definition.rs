//! Aether's version-1 mode contract (aether-tui/src/view_modes.rs).
//! Kept local because Archon does not depend on Aether's TUI implementation.

use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Density {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Category {
    UserText,
    AssistantText,
    Delegation,
    AgentMessage,
    Completion,
    Status,
    Question,
    Approval,
    Error,
    Tool,
    Reasoning,
    Memory,
    Compaction,
    Usage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    name: Option<String>,
    role: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    version: u32,
    label: Option<String>,
    description: Option<String>,
    #[serde(default, rename = "density")]
    _density: Density,
    show: Vec<Category>,
    scope: Option<Scope>,
}

pub(super) fn validate(text: &str) -> anyhow::Result<()> {
    let definition: Definition = toml::from_str(text)?;
    anyhow::ensure!(definition.version == 1, "version must be 1");
    let mut seen = std::collections::BTreeSet::new();
    anyhow::ensure!(
        definition.show.iter().all(|kind| seen.insert(kind)),
        "duplicate category in show"
    );
    let scope = definition.scope.as_ref();
    for (key, value, limit) in [
        ("label", definition.label.as_deref(), 64),
        ("description", definition.description.as_deref(), 160),
        ("scope.name", scope.and_then(|s| s.name.as_deref()), 128),
        ("scope.role", scope.and_then(|s| s.role.as_deref()), 128),
    ] {
        if let Some(value) = value {
            anyhow::ensure!(
                !value.trim().is_empty()
                    && value.chars().count() <= limit
                    && !value
                        .chars()
                        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}')),
                "{key} must be nonblank, single-line, without controls, and at most {limit} characters"
            );
        }
    }
    anyhow::ensure!(
        !scope.is_some_and(|s| s.name.is_none() && s.role.is_none()),
        "scope must specify name or role"
    );
    Ok(())
}
