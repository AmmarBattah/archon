//! `bench/tasks/<id>/task.toml` (ARCHITECTURE §15).
//!
//! A task directory holds `task.toml`, one fixture directory per repository,
//! `grade.sh`, and grader-only material the agent never sees (`hidden/`,
//! `reference.patch`, `reference.answer`).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use archon_core::repo::Repo;
use serde::Deserialize;

/// The fixture directory name of single-repository tasks.
pub const DEFAULT_FIXTURE: &str = "fixture";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskFile {
    pub id: String,
    pub title: String,
    /// What the operator would type; identical for every arm.
    pub prompt: String,
    pub timeout_minutes: u32,
    /// Pipeline hint for the `archon` arm's report; the orchestrator still
    /// chooses.
    #[serde(default)]
    pub pipeline: Option<String>,
    /// Fixture directories (under the task dir) that become separate repos;
    /// default `["fixture"]`.
    #[serde(default = "default_repos")]
    pub repos: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_repos() -> Vec<String> {
    vec![DEFAULT_FIXTURE.into()]
}

/// One fixture directory and the repository it becomes in a trial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// Source directory under the task.
    pub dir: PathBuf,
    /// Repository (and trial directory) name: the task id for the single
    /// `fixture` directory, else the directory name.
    pub name: String,
}

impl Fixture {
    /// `BENCH_RESULT_<NAME>`: where the grader finds this repository.
    pub fn env_var(&self) -> String {
        let upper: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("BENCH_RESULT_{upper}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchTask {
    pub file: TaskFile,
    pub dir: PathBuf,
}

impl BenchTask {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join("task.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file: TaskFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if file.id != dir_name {
            bail!(
                "{}: id {:?} must equal the directory name",
                path.display(),
                file.id
            );
        }
        validate(&file).with_context(|| format!("invalid {}", path.display()))?;
        if !dir.join("grade.sh").is_file() {
            bail!("{}: missing grade.sh", dir.display());
        }
        for repo in &file.repos {
            if !dir.join(repo).is_dir() {
                bail!("{}: missing fixture directory {repo}", dir.display());
            }
        }
        Ok(Self {
            file,
            dir: dir.to_path_buf(),
        })
    }

    /// Every task directory under `root`, sorted by id.
    pub fn load_all(root: &Path) -> anyhow::Result<Vec<Self>> {
        let mut tasks = Vec::new();
        let entries =
            std::fs::read_dir(root).with_context(|| format!("reading {}", root.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path.join("task.toml").is_file() {
                tasks.push(Self::load(&path)?);
            }
        }
        tasks.sort_by(|a, b| a.file.id.cmp(&b.file.id));
        Ok(tasks)
    }

    /// Keep the tasks named in `only` (all when empty); unknown ids fail.
    pub fn select(tasks: Vec<Self>, only: &[String]) -> anyhow::Result<Vec<Self>> {
        if only.is_empty() {
            return Ok(tasks);
        }
        for id in only {
            ensure!(
                tasks.iter().any(|t| &t.file.id == id),
                "unknown task {id:?}; `archon-bench list` shows the suite"
            );
        }
        Ok(tasks
            .into_iter()
            .filter(|t| only.contains(&t.file.id))
            .collect())
    }

    pub fn fixtures(&self) -> Vec<Fixture> {
        self.file
            .repos
            .iter()
            .map(|repo| Fixture {
                dir: self.dir.join(repo),
                name: if repo == DEFAULT_FIXTURE {
                    self.file.id.clone()
                } else {
                    repo.clone()
                },
            })
            .collect()
    }

    pub fn is_multi_repo(&self) -> bool {
        self.file.repos.len() > 1
    }

    pub fn grade_script(&self) -> PathBuf {
        self.dir.join("grade.sh")
    }

    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.file.timeout_minutes) * 60)
    }
}

fn validate(file: &TaskFile) -> anyhow::Result<()> {
    ensure!(!file.prompt.trim().is_empty(), "the prompt is empty");
    ensure!(!file.title.trim().is_empty(), "the title is empty");
    ensure!(
        (1..=24 * 60).contains(&file.timeout_minutes),
        "timeout_minutes must be between 1 and 1440"
    );
    ensure!(
        !file.repos.is_empty(),
        "repos must name at least one fixture"
    );
    ensure!(
        Repo::valid_name(&file.id),
        "id {:?} is not a valid repository name",
        file.id
    );
    let mut seen = std::collections::BTreeSet::new();
    for repo in &file.repos {
        ensure!(
            Repo::valid_name(repo),
            "fixture {repo:?} is not a valid repository name"
        );
        ensure!(seen.insert(repo), "fixture {repo:?} is listed twice");
    }
    ensure!(
        file.repos.len() == 1 || !seen.contains(&DEFAULT_FIXTURE.to_owned()),
        "multi-repo tasks name their fixtures; {DEFAULT_FIXTURE:?} is for single-repo tasks"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_task(root: &Path, id: &str, toml: &str, repos: &[&str]) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("task.toml"), toml).unwrap();
        std::fs::write(dir.join("grade.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        for repo in repos {
            std::fs::create_dir_all(dir.join(repo)).unwrap();
        }
        dir
    }

    fn scratch() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(".work");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("task-test-")
            .tempdir_in(base)
            .unwrap()
    }

    #[test]
    fn bundled_tasks_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks");
        let tasks = BenchTask::load_all(&root).unwrap();
        assert!(tasks.len() >= 8, "the suite ships at least 8 tasks");
        assert!(tasks.iter().all(|t| t.file.timeout_minutes > 0));
        let multi = tasks.iter().filter(|t| t.is_multi_repo()).count();
        assert!(multi >= 2, "at least two multi-repo tasks, found {multi}");
        let ids: Vec<&str> = tasks.iter().map(|t| t.file.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "load_all sorts by id");
    }

    #[test]
    fn single_repo_fixtures_are_named_after_the_task() {
        let tmp = scratch();
        let dir = write_task(
            tmp.path(),
            "fix-it",
            "id = \"fix-it\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 5\n",
            &["fixture"],
        );
        let task = BenchTask::load(&dir).unwrap();
        let fixtures = task.fixtures();
        assert_eq!(fixtures.len(), 1);
        assert_eq!(fixtures[0].name, "fix-it");
        assert_eq!(fixtures[0].env_var(), "BENCH_RESULT_FIX_IT");
        assert!(!task.is_multi_repo());
    }

    #[test]
    fn multi_repo_fixtures_keep_their_names() {
        let tmp = scratch();
        let toml = "id = \"two\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 5\n\
                    repos = [\"api\", \"web-client\"]\n";
        let dir = write_task(tmp.path(), "two", toml, &["api", "web-client"]);
        let task = BenchTask::load(&dir).unwrap();
        let names: Vec<String> = task.fixtures().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["api", "web-client"]);
        assert_eq!(task.fixtures()[1].env_var(), "BENCH_RESULT_WEB_CLIENT");
    }

    #[test]
    fn invalid_tasks_are_refused_with_the_reason() {
        let tmp = scratch();
        let cases = [
            (
                "wrong-id",
                "id = \"other\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 5\n",
                "directory name",
            ),
            (
                "no-prompt",
                "id = \"no-prompt\"\ntitle = \"t\"\nprompt = \" \"\ntimeout_minutes = 5\n",
                "prompt is empty",
            ),
            (
                "zero-time",
                "id = \"zero-time\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 0\n",
                "timeout_minutes",
            ),
            (
                "dup",
                "id = \"dup\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 5\nrepos = [\"a\", \"a\"]\n",
                "twice",
            ),
            (
                "typo",
                "id = \"typo\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 5\ntimeout = 3\n",
                "parsing",
            ),
        ];
        for (id, toml, needle) in cases {
            let dir = write_task(tmp.path(), id, toml, &["fixture", "a"]);
            let err = format!("{:#}", BenchTask::load(&dir).unwrap_err());
            assert!(err.contains(needle), "{id}: {err}");
        }
        let missing = write_task(
            tmp.path(),
            "missing",
            "id = \"missing\"\ntitle = \"t\"\nprompt = \"p\"\ntimeout_minutes = 5\n",
            &[],
        );
        let err = format!("{:#}", BenchTask::load(&missing).unwrap_err());
        assert!(err.contains("missing fixture directory"), "{err}");
    }

    #[test]
    fn select_filters_and_rejects_unknown_ids() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks");
        let all = BenchTask::load_all(&root).unwrap();
        let first = all[0].file.id.clone();
        let picked = BenchTask::select(all.clone(), std::slice::from_ref(&first)).unwrap();
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].file.id, first);
        assert_eq!(
            BenchTask::select(all.clone(), &[]).unwrap().len(),
            all.len()
        );
        assert!(BenchTask::select(all, &["nope".into()]).is_err());
    }
}
