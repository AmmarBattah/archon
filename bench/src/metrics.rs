//! One JSON line per trial in `results.jsonl` (ARCHITECTURE §15).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use archon_core::Timestamp;
use archon_core::autonomy::ReviewLevel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    /// The full Archon stack: orchestrator + pipelines.
    Archon,
    /// One plain Aether root agent.
    Aether,
    /// `claude -p`.
    Claude,
    /// `codex exec`.
    Codex,
}

impl Arm {
    pub const ALL: [Arm; 4] = [Arm::Archon, Arm::Aether, Arm::Claude, Arm::Codex];

    pub fn name(self) -> &'static str {
        match self {
            Self::Archon => "archon",
            Self::Aether => "aether",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

impl fmt::Display for Arm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Arm {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|arm| arm.name() == s.trim())
            .ok_or_else(|| {
                anyhow::anyhow!("unknown arm {s:?}; expected archon, aether, claude or codex")
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrialResult {
    pub task: String,
    pub arm: Arm,
    pub trial: u32,
    pub model: String,
    pub effort: Option<String>,
    /// Grader exit status 0.
    pub success: bool,
    /// Optional grader JSON `{"score": 0..1}`.
    pub score: Option<f64>,
    /// From handing over the prompt to the arm finishing; excludes fixture
    /// setup, daemon startup (`setup_secs`) and grading.
    pub wall_secs: f64,
    /// Every prompt token, cached or not, so arms compare like for like.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// The part of `input_tokens` served from a prompt cache.
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    /// Dollars the harness reported as billed (or list price, for `claude`).
    pub cost_usd: Option<f64>,
    /// Subscription estimates, or `--price` applied to the token counts.
    pub estimated_cost_usd: Option<f64>,
    /// Why Aether called `cost_usd` incomplete; present means it is a floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_gap: Option<String>,
    /// Messages the operator would have had to read.
    pub operator_messages: u32,
    /// Harness failure (not a task failure): the trial did not run cleanly.
    pub error: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    /// Archon only: daemon start until the orchestrator was ready.
    #[serde(default)]
    pub setup_secs: Option<f64>,
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    #[serde(default)]
    pub finished_at: Option<Timestamp>,
    /// Grading remarks, such as a result branch left out on a merge conflict.
    #[serde(default)]
    pub note: Option<String>,
    /// Captured queue work; absent means unknown, an empty list means no work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archon_work: Option<Vec<ArchonItem>>,
    /// Timed-out trials only: the grader's result with the last committed
    /// branch of each timed-out item added. It never changes `success`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graded_on_timeout: Option<TimeoutGrade>,
}

/// The grader's verdict on a timed-out trial's committed work, kept apart
/// from the trial's own verdict so it is never counted as a pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeoutGrade {
    pub success: bool,
    pub score: Option<f64>,
}

/// Routing observed at capture time, without attributing the trial verdict to items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchonItem {
    pub pipeline: String,
    #[serde(default)]
    pub run_pipelines: Vec<String>,
    /// Selected review policy, not proof that reviewers ran or passed.
    pub review: Option<ReviewLevel>,
}

impl TrialResult {
    /// A failed trial the arm never got a fair run at: the harness or the
    /// provider broke (`error` set). Kept out of pass rates and metrics.
    pub fn is_infra_failure(&self) -> bool {
        !self.success && self.error.is_some()
    }

    /// The cost to compare arms by: billed dollars when any were reported,
    /// else the estimate. `None` means nobody could price the trial.
    pub fn effective_cost_usd(&self) -> Option<f64> {
        match (self.cost_usd, self.estimated_cost_usd) {
            (Some(billed), _) if billed > 0.0 => Some(billed),
            (billed, Some(estimate)) => Some(estimate.max(billed.unwrap_or(0.0))),
            (billed, None) => billed,
        }
    }
}

/// `run.json`: everything needed to reproduce and judge a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunManifest {
    pub stamp: String,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    /// `git rev-parse HEAD` of the Archon checkout the bench ran from.
    pub archon_commit: Option<String>,
    /// Program → first line of `--version` (and aetherd's daemon version).
    pub versions: BTreeMap<String, String>,
    /// Arm → the model label recorded in its results.
    pub models: BTreeMap<String, String>,
    pub effort: String,
    pub arms: Vec<Arm>,
    pub tasks: Vec<String>,
    pub trials: u32,
    pub parallel: usize,
    pub timeout_factor: f64,
    pub max_usd: f64,
    pub max_trials: usize,
    pub planned: usize,
    pub completed: usize,
    pub skipped: usize,
    pub spent_usd: f64,
    pub unpriced_trials: u32,
    /// Why the run ended early (budget exhausted), when it did.
    pub stopped_reason: Option<String>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn result(cost: Option<f64>, estimate: Option<f64>) -> TrialResult {
        TrialResult {
            task: "t".into(),
            arm: Arm::Claude,
            trial: 1,
            model: "m".into(),
            effort: None,
            success: true,
            score: None,
            wall_secs: 1.0,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            cost_usd: cost,
            estimated_cost_usd: estimate,
            cost_gap: None,
            operator_messages: 1,
            error: None,
            timed_out: false,
            setup_secs: None,
            started_at: None,
            finished_at: None,
            note: None,
            archon_work: None,
            graded_on_timeout: None,
        }
    }

    #[test]
    fn arms_round_trip_through_their_names() {
        for arm in Arm::ALL {
            assert_eq!(arm.name().parse::<Arm>().unwrap(), arm);
            let json = serde_json::to_string(&arm).unwrap();
            assert_eq!(json, format!("\"{arm}\""));
        }
        assert!("gpt".parse::<Arm>().is_err());
    }

    #[test]
    fn effective_cost_prefers_billed_then_estimate() {
        assert_eq!(result(Some(2.0), Some(5.0)).effective_cost_usd(), Some(2.0));
        assert_eq!(result(Some(0.0), Some(5.0)).effective_cost_usd(), Some(5.0));
        assert_eq!(result(None, Some(1.5)).effective_cost_usd(), Some(1.5));
        assert_eq!(result(Some(0.0), None).effective_cost_usd(), Some(0.0));
        assert_eq!(result(None, None).effective_cost_usd(), None);
    }

    #[test]
    fn old_result_lines_without_new_fields_still_parse() {
        let line = r#"{"task":"t","arm":"codex","trial":2,"model":"m","effort":null,
            "success":false,"score":null,"wall_secs":3.5,"input_tokens":10,
            "output_tokens":2,"cost_usd":null,"estimated_cost_usd":null,
            "operator_messages":1,"error":null}"#;
        let parsed: TrialResult = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.arm, Arm::Codex);
        assert!(!parsed.timed_out && parsed.cached_input_tokens.is_none());
        assert!(parsed.archon_work.is_none() && parsed.graded_on_timeout.is_none());
        assert_eq!(parsed.cost_gap, None);
    }

    #[test]
    fn cost_gap_round_trips_and_stays_absent_when_unused() {
        let plain = serde_json::to_string(&result(Some(1.0), None)).unwrap();
        assert!(!plain.contains("cost_gap"), "{plain}");
        let mut floor = result(Some(1.0), None);
        floor.cost_gap = Some("interrupted".into());
        let parsed: TrialResult =
            serde_json::from_str(&serde_json::to_string(&floor).unwrap()).unwrap();
        assert_eq!(parsed.cost_gap.as_deref(), Some("interrupted"));
    }

    #[test]
    fn graded_on_timeout_round_trips_and_stays_absent_when_unused() {
        let plain = serde_json::to_string(&result(None, None)).unwrap();
        assert!(!plain.contains("graded_on_timeout"), "{plain}");
        let mut late = result(None, None);
        late.success = false;
        late.timed_out = true;
        late.graded_on_timeout = Some(TimeoutGrade {
            success: true,
            score: Some(1.0),
        });
        let parsed: TrialResult =
            serde_json::from_str(&serde_json::to_string(&late).unwrap()).unwrap();
        assert_eq!(parsed, late);
        assert!(!parsed.success, "the verdict is untouched");
    }
}
