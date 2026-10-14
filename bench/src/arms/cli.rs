//! The `claude` and `codex` arms: one CLI process in the agent's working
//! directory. Operator-visible messages are 1 (the final result).

use std::path::Path;

use crate::arms::{ArmOutcome, TrialInput, claude_command, codex_command};
use crate::grade::ResultSource;
use crate::metrics::Arm;
use crate::process;
use crate::usage::{parse_claude_json, parse_codex_jsonl};

/// How much of a failing CLI's stderr an error message carries.
const STDERR_TAIL_CHARS: usize = 2_000;

pub async fn run_claude(input: &TrialInput<'_>) -> ArmOutcome {
    let cwd = input.layout.agent_cwd(input.task);
    let spec = claude_command(
        input.settings,
        &input.task.file.prompt,
        &cwd,
        &input.layout.workspace,
    );
    let (stdout, stderr) = log_paths(input, Arm::Claude);
    let run = match process::run(&spec, &stdout, &stderr, input.timeout).await {
        Ok(run) => run,
        Err(e) => return ArmOutcome::failed(format!("{e:#}"), ResultSource::Workspace),
    };
    let text = std::fs::read_to_string(&stdout).unwrap_or_default();
    let mut outcome = base_outcome(run.elapsed.as_secs_f64(), run.timed_out);
    match parse_claude_json(&text) {
        Ok(parsed) => {
            outcome.usage = Some(input.settings.price(Arm::Claude, parsed.usage));
            // Hitting the turn limit is the agent's failure; anything else
            // flagged as an error (auth, API, crash) is the harness's.
            if parsed.is_error && parsed.subtype != "error_max_turns" {
                outcome.error = Some(format!(
                    "claude reported {}: {}",
                    parsed.subtype, parsed.result
                ));
            }
            outcome.answer = parsed.result;
        }
        Err(_) if run.timed_out => {}
        Err(e) => {
            outcome.error = Some(format!(
                "claude exited {:?} without a result: {e:#}; stderr: {}",
                run.status,
                tail(&stderr)
            ));
        }
    }
    outcome
}

pub async fn run_codex(input: &TrialInput<'_>) -> ArmOutcome {
    let cwd = input.layout.agent_cwd(input.task);
    let spec = codex_command(
        input.settings,
        &input.task.file.prompt,
        &cwd,
        &input.layout.workspace,
    );
    let (stdout, stderr) = log_paths(input, Arm::Codex);
    let run = match process::run(&spec, &stdout, &stderr, input.timeout).await {
        Ok(run) => run,
        Err(e) => return ArmOutcome::failed(format!("{e:#}"), ResultSource::Workspace),
    };
    let text = std::fs::read_to_string(&stdout).unwrap_or_default();
    let parsed = parse_codex_jsonl(&text);
    let mut outcome = base_outcome(run.elapsed.as_secs_f64(), run.timed_out);
    outcome.usage = parsed.usage.map(|u| input.settings.price(Arm::Codex, u));
    outcome.answer = parsed.last_message.unwrap_or_default();
    if !run.timed_out && !run.success() {
        let why = if parsed.errors.is_empty() {
            tail(&stderr)
        } else {
            parsed.errors.join("; ")
        };
        outcome.error = Some(format!("codex exited {:?}: {why}", run.status));
    }
    outcome
}

fn base_outcome(wall_secs: f64, timed_out: bool) -> ArmOutcome {
    assert!(wall_secs >= 0.0, "elapsed time is never negative");
    ArmOutcome {
        usage: None,
        answer: String::new(),
        operator_messages: 1,
        wall_secs,
        setup_secs: None,
        timed_out,
        error: None,
        source: ResultSource::Workspace,
        timeout_source: None,
    }
}

fn log_paths(input: &TrialInput<'_>, arm: Arm) -> (std::path::PathBuf, std::path::PathBuf) {
    let logs = &input.layout.logs;
    (
        logs.join(format!("{arm}.stdout")),
        logs.join(format!("{arm}.stderr")),
    )
}

/// The end of a log file, for error messages.
pub fn tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let text = text.trim();
    let count = text.chars().count();
    if count <= STDERR_TAIL_CHARS {
        return text.to_owned();
    }
    text.chars().skip(count - STDERR_TAIL_CHARS).collect()
}
