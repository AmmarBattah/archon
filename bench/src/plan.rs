//! What a run will do, decided before anything is spent: the trial order,
//! the guards (`--max-trials`, `--max-usd`), and the `--dry-run` listing
//! with every arm's exact command line.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

use crate::arms::{
    ArmSettings, TrialInput, aether_request, archon_config_toml, archon_daemon_command,
    claude_command, codex_command,
};
use crate::metrics::{Arm, CostBound, TrialResult};
use crate::task::BenchTask;
use crate::workspace::TrialLayout;

#[derive(Debug, Clone, PartialEq)]
pub struct RunOptions {
    pub arms: Vec<Arm>,
    pub trials: u32,
    pub parallel: usize,
    pub max_usd: f64,
    pub max_trials: usize,
    pub timeout_factor: f64,
    pub work_dir: PathBuf,
    pub results_dir: PathBuf,
    pub stamp: String,
    /// Keep each trial workspace after grading (logs always stay).
    pub keep_workspaces: bool,
}

impl RunOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(!self.arms.is_empty(), "choose at least one arm");
        ensure!(self.trials >= 1, "--trials must be at least 1");
        ensure!(self.parallel >= 1, "--parallel must be at least 1");
        ensure!(
            self.max_usd.is_finite() && self.max_usd > 0.0,
            "--max-usd must be a positive number"
        );
        ensure!(
            self.timeout_factor.is_finite() && self.timeout_factor > 0.0,
            "--timeout-factor must be positive"
        );
        let mut unique = self.arms.clone();
        unique.sort_unstable();
        unique.dedup();
        ensure!(unique.len() == self.arms.len(), "an arm is listed twice");
        Ok(())
    }

    /// The run's traverse-only directory holding every trial workspace.
    pub fn run_work_dir(&self) -> PathBuf {
        self.work_dir.join(&self.stamp)
    }

    /// Where a trial's logs, answer and result patches are kept.
    pub fn trial_logs_dir(&self, task: &str, arm: Arm, trial: u32) -> PathBuf {
        self.run_results_dir()
            .join("trials")
            .join(task)
            .join(arm.name())
            .join(trial.to_string())
            .join("logs")
    }

    pub fn run_results_dir(&self) -> PathBuf {
        self.results_dir.join(&self.stamp)
    }

    pub fn timeout_for(&self, task: &BenchTask) -> Duration {
        task.timeout().mul_f64(self.timeout_factor)
    }
}

/// One planned trial: an index into the task list, an arm, a 1-based number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialSpec {
    pub task: usize,
    pub arm: Arm,
    pub trial: u32,
}

/// Trial-major, then task, then arm: the arms interleave, so drift over a
/// long run (rate limits, provider load, time of day) hits them alike.
pub fn plan(task_count: usize, arms: &[Arm], trials: u32) -> Vec<TrialSpec> {
    let mut specs = Vec::with_capacity(task_count * arms.len() * trials as usize);
    for trial in 1..=trials {
        for task in 0..task_count {
            specs.extend(arms.iter().map(|&arm| TrialSpec { task, arm, trial }));
        }
    }
    assert_eq!(specs.len(), task_count * arms.len() * trials as usize);
    specs
}

/// `--max-trials` refuses an oversized plan before anything runs.
pub fn check_plan(specs: &[TrialSpec], options: &RunOptions) -> anyhow::Result<()> {
    ensure!(!specs.is_empty(), "the plan is empty: no tasks selected");
    ensure!(
        specs.len() <= options.max_trials,
        "the plan has {} trials but --max-trials is {}; raise it or narrow --only/--arms/--trials",
        specs.len(),
        options.max_trials
    );
    Ok(())
}

/// Admission stops at the recorded limit; up to `--parallel` in-flight trials finish.
/// Even serial runs can overshoot. Unpriced trials count as $0 and are reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub limit_usd: f64,
    pub spent_usd: f64,
    /// Some counted trial's cost was reported incomplete.
    gapped: bool,
    /// Some counted trial's cost is a substituted estimate.
    estimated: bool,
    pub unpriced_trials: u32,
}

impl Budget {
    pub fn new(limit_usd: f64) -> Self {
        assert!(limit_usd > 0.0, "a budget must be positive");
        Self {
            limit_usd,
            spent_usd: 0.0,
            gapped: false,
            estimated: false,
            unpriced_trials: 0,
        }
    }

    pub fn allows_another(&self) -> bool {
        self.spent_usd < self.limit_usd
    }

    pub fn spent_bound(&self) -> CostBound {
        CostBound::from_flags(self.gapped, self.estimated)
    }

    pub fn record(&mut self, result: &TrialResult) {
        match result.effective_cost_usd() {
            Some(cost) => {
                assert!(cost >= 0.0, "a trial cannot earn money");
                self.spent_usd += cost;
                self.gapped |= result.cost_is_floor();
                self.estimated |= result.cost_is_estimate();
            }
            None => self.unpriced_trials += 1,
        }
    }
}

/// The `--dry-run` listing: plan, guards, and each (task, arm) command.
pub fn render_dry_run(
    tasks: &[BenchTask],
    specs: &[TrialSpec],
    settings: &ArmSettings,
    options: &RunOptions,
    versions: &[(Arm, String)],
) -> anyhow::Result<String> {
    assert_eq!(
        specs.len(),
        tasks.len() * options.arms.len() * options.trials as usize,
        "the plan does not cover every (task, arm, trial)"
    );
    let mut out = render_header(tasks.len(), specs.len(), settings, options, versions)?;
    for task in tasks {
        let timeout = options.timeout_for(task);
        writeln!(
            out,
            "\n== {} ({}; timeout {}s; repos {})",
            task.file.id,
            task.file.title,
            timeout.as_secs(),
            task.file.repos.join(",")
        )?;
        for &arm in &options.arms {
            let layout = TrialLayout::new(
                options.run_work_dir().join("<random>"),
                options.run_work_dir().join("<random-grade>"),
                options.trial_logs_dir(&task.file.id, arm, 1),
            );
            let input = TrialInput {
                task,
                layout: &layout,
                settings,
                timeout,
                key: "<stamp>-<task>-<arm>-<trial>",
            };
            writeln!(out, "  [{arm}] x{} trials:", options.trials)?;
            for line in describe_arm(arm, &input)? {
                writeln!(out, "    {line}")?;
            }
        }
    }
    Ok(out)
}

/// The plan, guard and arm lines that open the `--dry-run` listing.
fn render_header(
    task_count: usize,
    trial_count: usize,
    settings: &ArmSettings,
    options: &RunOptions,
    versions: &[(Arm, String)],
) -> anyhow::Result<String> {
    assert!(!options.arms.is_empty(), "a plan needs at least one arm");
    assert!(
        versions.iter().all(|(arm, _)| options.arms.contains(arm)),
        "versions for arms outside the plan"
    );
    let mut out = String::new();
    writeln!(
        out,
        "plan: {} tasks x {} arms x {} trials = {} trials (--max-trials {}, --max-usd {:.2}, --parallel {})",
        task_count,
        options.arms.len(),
        options.trials,
        trial_count,
        options.max_trials,
        options.max_usd,
        options.parallel
    )?;
    writeln!(
        out,
        "workspaces: {}/<random> (0711, {})",
        options.run_work_dir().display(),
        if options.keep_workspaces {
            "kept"
        } else {
            "deleted after grading"
        }
    )?;
    writeln!(out, "results:    {}", options.run_results_dir().display())?;
    writeln!(out, "\narms (effort {}):", settings.effort)?;
    for &arm in &options.arms {
        let version = versions
            .iter()
            .find(|(a, _)| *a == arm)
            .map_or("", |(_, v)| v.as_str());
        writeln!(
            out,
            "  {arm:<7} model {:<36} {version}",
            settings.model_label(arm)
        )?;
    }
    Ok(out)
}

/// The exact actions an arm takes for one trial, as printable lines.
pub fn describe_arm(arm: Arm, input: &TrialInput<'_>) -> anyhow::Result<Vec<String>> {
    let cwd = input.layout.agent_cwd(input.task);
    let prompt = &input.task.file.prompt;
    Ok(match arm {
        Arm::Claude => vec![format!(
            "$ {}",
            claude_command(input.settings, prompt, &cwd, &input.layout.workspace).render()
        )],
        Arm::Codex => vec![format!(
            "$ {}",
            codex_command(input.settings, prompt, &cwd, &input.layout.workspace).render()
        )],
        Arm::Aether => {
            let request = aether_request(input, archon_core::now());
            vec![format!(
                "aether create_agent {}",
                serde_json::to_string(&request)?
            )]
        }
        Arm::Archon => describe_archon(input)?,
    })
}

fn describe_archon(input: &TrialInput<'_>) -> anyhow::Result<Vec<String>> {
    let Some(sockets) = &input.settings.sockets else {
        return Ok(vec![
            "refused: no private sandbox, so a live run starts no archond".to_owned(),
        ]);
    };
    let daemon = archon_daemon_command(input.settings, &input.layout.archon, &sockets.archon);
    let config = archon_config_toml(input.settings)?.replace('\n', "\n      ");
    let mut lines = vec![
        format!(
            "write {}/config/config.toml:\n      {config}",
            input.layout.archon.display()
        ),
        format!("$ {}", daemon.render()),
    ];
    for fixture in input.task.fixtures() {
        lines.push(format!(
            "repo_add {} --name {} --ship local --base main",
            input.layout.repo(&fixture).display(),
            fixture.name
        ));
    }
    lines.push(format!(
        "say {}",
        serde_json::to_string(&input.task.file.prompt)?
    ));
    lines.push("wait: orchestrator idle, every item terminal or blocked, quiet for 45s".into());
    Ok(lines)
}

/// `$XDG_DATA_HOME/archon-bench/work`, else `~/.local/share/archon-bench/work`:
/// outside the checkout, so agents cannot walk up to the task suite, and
/// off the small shared /tmp.
pub fn default_work_dir() -> anyhow::Result<PathBuf> {
    work_dir_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

fn work_dir_from(
    xdg_data_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> anyhow::Result<PathBuf> {
    let absolute = |value: std::ffi::OsString| {
        let path = PathBuf::from(value);
        path.is_absolute().then_some(path)
    };
    let data_home = xdg_data_home
        .and_then(absolute)
        .or_else(|| home.and_then(absolute).map(|h| h.join(".local/share")))
        .context("neither XDG_DATA_HOME nor HOME is an absolute path; pass --work-dir")?;
    let dir = data_home.join("archon-bench").join("work");
    assert!(dir.is_absolute(), "only absolute candidates are accepted");
    Ok(dir)
}

pub fn default_results_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("results")
}

#[cfg(test)]
mod tests;
