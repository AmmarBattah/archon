//! What the operator sees. The chat is a filtered feed; these are the only
//! item kinds it renders.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::aether_ids::AetherQuestionRef;
use crate::id::{FeedItemId, NotificationId, PipelineRunId, QuestionId, QueueItemId, ReportId};

/// The orchestrator's `notify(kind=...)` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// Needs an operator answer; pairs with a `Question`.
    Question,
    /// A choice the orchestrator made that the operator should know about.
    Decision,
    /// Work cannot proceed without operator action.
    Blocker,
    /// A queue item or run finished without requiring operator action.
    /// Terminal failures requiring intervention use `Blocker` instead.
    Outcome,
    /// Hidden by default; shown when the operator asks for progress.
    Progress,
}

impl NotificationKind {
    /// Whether the default (quiet) chat renders this kind.
    pub fn shown_by_default(self) -> bool {
        !matches!(self, Self::Progress)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub id: NotificationId,
    pub kind: NotificationKind,
    pub text: String,
    pub item: Option<QueueItemId>,
    pub run: Option<PipelineRunId>,
    pub at: Timestamp,
}

/// A question the orchestrator asked without blocking (Archon MCP
/// `ask_operator`). Aether's own `ask_user` questions are mirrored here too
/// so one place holds everything awaiting an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: QuestionId,
    /// The operator's `Q<n>`, the same in every listing and chat. The store
    /// assigns it when recording the question and never reuses it; 0 before.
    #[serde(default)]
    pub number: u64,
    pub text: String,
    /// Suggested answers, when the asker offered some.
    pub options: Vec<String>,
    pub item: Option<QueueItemId>,
    /// Set when the question came from Aether's `ask_user`; the answer then
    /// goes back through `reply_to_question` instead of a trigger.
    pub aether: Option<AetherQuestionRef>,
    pub asked_at: Timestamp,
    pub answer: Option<String>,
    pub answered_at: Option<Timestamp>,
}

impl Question {
    pub fn is_open(&self) -> bool {
        self.answer.is_none()
    }
}

/// One line of the operator chat. Persisted so `archon chat` can replay.
/// Tagged `entry` because the wrapped `Notification` already has a `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum FeedEntry {
    Operator {
        text: String,
    },
    /// Final text of an orchestrator turn that answered the operator.
    Orchestrator {
        text: String,
    },
    Notification(Notification),
    Question(Question),
    Answer {
        question: QuestionId,
        text: String,
    },
    /// A report published for review (Lavish URL or plain file path).
    Report {
        report: ReportId,
        title: String,
        location: String,
        item: Option<QueueItemId>,
    },
    /// Daemon-level notices: start/stop, orchestrator restarted, caps hit.
    System {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedItem {
    pub id: FeedItemId,
    /// Gapless per-store sequence for `SubscribeFeed { after }`.
    pub seq: u64,
    pub at: Timestamp,
    pub entry: FeedEntry,
}

impl FeedItem {
    /// The quiet-chat filter. `verbose` also shows progress notifications.
    pub fn visible(&self, verbose: bool) -> bool {
        match &self.entry {
            FeedEntry::Notification(n) => verbose || n.kind.shown_by_default(),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress_item() -> FeedItem {
        FeedItem {
            id: FeedItemId::new(),
            seq: 1,
            at: crate::now(),
            entry: FeedEntry::Notification(Notification {
                id: NotificationId::new(),
                kind: NotificationKind::Progress,
                text: "50%".into(),
                item: None,
                run: None,
                at: crate::now(),
            }),
        }
    }

    #[test]
    fn progress_is_hidden_unless_verbose() {
        let item = progress_item();
        assert!(!item.visible(false));
        assert!(item.visible(true));
    }

    #[test]
    fn notification_entry_round_trips_through_json() {
        let item = progress_item();
        let json = serde_json::to_string(&item).expect("serialize feed item");
        assert!(json.contains(r#""entry":"notification""#), "{json}");
        let back: FeedItem = serde_json::from_str(&json).expect("deserialize feed item");
        assert_eq!(back, item);
    }
}
