//! Child processes with a wall-clock limit. Each runs in its own process
//! group so a timeout also stops the tools it spawned (shells, test
//! runners, language servers), and so do background processes it leaves
//! behind when it exits: nothing an agent started may outlive its trial and
//! touch the grading copy or slow the next trial down.

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;

/// How long a timed-out group gets between SIGTERM and SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(10);

/// A fully specified command line; pure data so plans can print it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: Vec::new(),
            env_remove: Vec::new(),
        }
    }

    /// The process to spawn, in its own process group with stdin closed.
    pub fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        for name in &self.env_remove {
            command.env_remove(name);
        }
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }

    /// A copy-pasteable shell line: `(cd DIR && env -u X K=V prog args)`.
    pub fn render(&self) -> String {
        let mut words = Vec::new();
        if !self.env_remove.is_empty() || !self.env.is_empty() {
            words.push("env".to_owned());
        }
        for name in &self.env_remove {
            words.push(format!("-u {}", shell_quote(name)));
        }
        for (key, value) in &self.env {
            words.push(format!("{key}={}", shell_quote(value)));
        }
        words.push(shell_quote(&self.program.to_string_lossy()));
        words.extend(self.args.iter().map(|a| shell_quote(a)));
        format!(
            "(cd {} && {})",
            shell_quote(&self.cwd.to_string_lossy()),
            words.join(" ")
        )
    }
}

pub fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./=:,@%+".contains(c));
    if plain {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessOutcome {
    /// `None` when the process was killed after the timeout.
    pub status: Option<ExitStatus>,
    pub timed_out: bool,
    pub elapsed: Duration,
}

impl ProcessOutcome {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|s| s.success())
    }
}

/// Run `spec` with stdin closed and stdout/stderr written to the given
/// files; kill its whole process group once `timeout` passes, or once the
/// leader exits.
pub async fn run(
    spec: &CommandSpec,
    stdout: &Path,
    stderr: &Path,
    timeout: Duration,
) -> anyhow::Result<ProcessOutcome> {
    assert!(
        !timeout.is_zero(),
        "a zero timeout would kill every process"
    );
    assert!(
        spec.cwd.is_dir(),
        "{} is not a directory",
        spec.cwd.display()
    );
    let out =
        std::fs::File::create(stdout).with_context(|| format!("creating {}", stdout.display()))?;
    let err =
        std::fs::File::create(stderr).with_context(|| format!("creating {}", stderr.display()))?;
    let mut command = spec.command();
    command.stdout(out).stderr(err);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", spec.program.display()))?;
    let pid = child
        .id()
        .context("the child exited before its pid was read")?;
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => {
            let status = status.context("waiting for the child")?;
            let elapsed = started.elapsed();
            signal_group("KILL", pid).await?;
            Ok(ProcessOutcome {
                status: Some(status),
                timed_out: false,
                elapsed,
            })
        }
        Err(_) => {
            stop_group(pid, &mut child).await?;
            Ok(ProcessOutcome {
                status: None,
                timed_out: true,
                elapsed: started.elapsed(),
            })
        }
    }
}

/// SIGTERM the group, give the leader a grace period, then SIGKILL whatever
/// of the group is left.
async fn stop_group(pid: u32, child: &mut tokio::process::Child) -> anyhow::Result<()> {
    signal_group("TERM", pid).await?;
    let _ = tokio::time::timeout(TERM_GRACE, child.wait()).await;
    signal_group("KILL", pid).await?;
    child.wait().await.context("reaping the killed child")?;
    Ok(())
}

async fn signal_group(signal: &str, pid: u32) -> anyhow::Result<()> {
    assert!(pid > 1, "refusing to signal process group {pid}");
    // `kill` exits non-zero when the group is already gone, which is fine.
    tokio::process::Command::new("kill")
        .args([format!("-{signal}"), "--".to_owned(), format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| format!("sending SIG{signal} to process group {pid}"))?;
    Ok(())
}

/// First line of `program --version`, or why it could not be read.
pub fn version_of(program: &Path) -> String {
    match std::process::Command::new(program)
        .arg("--version")
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned(),
        Ok(out) => format!(
            "unknown ({} --version exited {})",
            program.display(),
            out.status
        ),
        Err(e) => format!("unavailable ({}: {e})", program.display()),
    }
}

pub fn redact_home(text: &str, home: Option<&str>) -> String {
    match home.filter(|home| home.starts_with('/') && home.len() > 1) {
        Some(home) => text.replace(home, "$HOME"),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_redact_home_in_output_and_diagnostics() {
        let home = "/users/operator";
        for text in [
            "tool 1.0 (/users/operator/bin/tool)",
            "unavailable (/users/operator/bin/tool: missing)",
            "unreachable: connecting to /users/operator/.local/share/archon-bench/run/aether.sock",
        ] {
            let redacted = redact_home(text, Some(home));
            assert!(!redacted.contains(home));
            assert_eq!(redacted, text.replace(home, "$HOME"));
        }
        for home in [None, Some(""), Some("/"), Some("relative")] {
            assert_eq!(redact_home("tool 1.0", home), "tool 1.0");
        }
    }

    #[test]
    fn quoting_leaves_plain_words_and_escapes_the_rest() {
        assert_eq!(shell_quote("--model"), "--model");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
        let mut spec = CommandSpec::new("claude", "/w d");
        spec.args = vec!["-p".into(), "fix it".into()];
        spec.env_remove = vec!["CLAUDECODE".into()];
        assert_eq!(
            spec.render(),
            "(cd '/w d' && env -u CLAUDECODE claude -p 'fix it')"
        );
    }

    #[tokio::test]
    async fn run_captures_output_and_status() {
        let tmp = crate::workspace::tests::scratch();
        let mut spec = CommandSpec::new("sh", tmp.path());
        spec.args = vec!["-c".into(), "echo out; echo err >&2; exit 3".into()];
        let (out, err) = (tmp.path().join("o"), tmp.path().join("e"));
        let outcome = run(&spec, &out, &err, Duration::from_secs(10))
            .await
            .unwrap();
        assert!(!outcome.timed_out);
        assert_eq!(outcome.status.unwrap().code(), Some(3));
        assert_eq!(std::fs::read_to_string(out).unwrap(), "out\n");
        assert_eq!(std::fs::read_to_string(err).unwrap(), "err\n");
    }

    #[tokio::test]
    async fn background_processes_die_with_their_leader() {
        let tmp = crate::workspace::tests::scratch();
        let marker = tmp.path().join("survived");
        let mut spec = CommandSpec::new("sh", tmp.path());
        let script = format!("(sleep 2; touch {}) & exit 0", marker.display());
        spec.args = vec!["-c".into(), script];
        let (out, err) = (tmp.path().join("o"), tmp.path().join("e"));
        let outcome = run(&spec, &out, &err, Duration::from_secs(10))
            .await
            .unwrap();
        assert!(outcome.success() && !outcome.timed_out);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "the leftover background job was killed");
    }

    #[tokio::test]
    async fn run_kills_the_whole_group_on_timeout() {
        let tmp = crate::workspace::tests::scratch();
        let marker = tmp.path().join("survived");
        let mut spec = CommandSpec::new("sh", tmp.path());
        let script = format!("(sleep 2; touch {}) & sleep 30", marker.display());
        spec.args = vec!["-c".into(), script];
        let (out, err) = (tmp.path().join("o"), tmp.path().join("e"));
        let outcome = run(&spec, &out, &err, Duration::from_millis(300))
            .await
            .unwrap();
        assert!(outcome.timed_out && !outcome.success());
        assert!(outcome.elapsed < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "the background child was killed too");
    }
}
