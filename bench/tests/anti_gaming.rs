//! Cheap cheats must not pass a grader: exiting early from the code under
//! test, editing the visible tests, switching tests off, and echoing or
//! scattering keywords into an answer.

use std::path::{Path, PathBuf};
use std::process::Command;

use archon_bench::grade::{self, GradeOutcome, ResultSource};
use archon_bench::task::BenchTask;
use archon_bench::verify::apply_reference;
use archon_bench::workspace::{self, BASE_TAG, TrialLayout};

/// A prepared workspace for one task, as an agent would receive it.
struct Trial {
    task: BenchTask,
    layout: TrialLayout,
    _scratch: tempfile::TempDir,
}

impl Trial {
    fn new(id: &str) -> Self {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let task = BenchTask::load(&manifest.join("tasks").join(id)).unwrap();
        let work = manifest.join(".work");
        std::fs::create_dir_all(&work).unwrap();
        let scratch = tempfile::Builder::new()
            .prefix("cheat-")
            .tempdir_in(&work)
            .unwrap();
        let layout = TrialLayout::under(scratch.path());
        workspace::prepare(&task, &layout).unwrap();
        Self {
            task,
            layout,
            _scratch: scratch,
        }
    }

    /// The reference solution applied; returns the reference answer.
    fn solved(id: &str) -> (Self, String) {
        let trial = Self::new(id);
        let answer = apply_reference(&trial.task, &trial.layout).unwrap();
        (trial, answer)
    }

    fn repo(&self, name: &str) -> PathBuf {
        let repo = self.layout.workspace.join(name);
        assert!(repo.join(".git").is_dir(), "{name} is not a fixture repo");
        repo
    }

    fn prepend(&self, repo: &str, file: &str, text: &str) {
        let path = self.repo(repo).join(file);
        let old = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{text}{old}")).unwrap();
    }

    /// Commit everything and move `bench-base` onto it, as a cheating agent
    /// could, so the grader's diff base no longer shows the edits.
    fn commit_and_retag(&self, repo: &str) {
        let repo = self.repo(repo);
        workspace::git(&repo, &["add", "-A"]).unwrap();
        workspace::git(&repo, &["commit", "-q", "-m", "cheat"]).unwrap();
        workspace::git(&repo, &["tag", "-f", BASE_TAG]).unwrap();
    }

    async fn grade(&self, answer: &str) -> GradeOutcome {
        std::fs::write(self.layout.answer_file(), answer).unwrap();
        grade::grade(&self.task, &self.layout, "cheat", &ResultSource::Workspace)
            .await
            .unwrap()
    }

    /// Grade and require a failure the grader explains with `reason`, so a
    /// test cannot pass because the grader broke for some other cause.
    async fn fails_because(&self, answer: &str, reason: &str) {
        let outcome = self.grade(answer).await;
        let log = std::fs::read_to_string(self.layout.logs.join("grade.stderr")).unwrap();
        assert!(
            !outcome.success,
            "{}: the cheat passed\n{log}",
            self.task.file.id
        );
        assert!(
            log.contains(reason),
            "{}: expected {reason:?} in\n{log}",
            self.task.file.id
        );
    }
}

fn exits_zero(dir: &Path, program: &str, args: &[&str]) -> bool {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status;
    status.success()
}

#[tokio::test]
async fn exiting_early_from_python_under_test_fails() {
    let trial = Trial::new("fix-off-by-one");
    trial.prepend("fix-off-by-one", "window.py", "import os\nos._exit(0)\n");
    let repo = trial.repo("fix-off-by-one");
    assert!(
        exits_zero(&repo, "python3", &["-m", "unittest", "-q"]),
        "the cheat makes the bare test command exit 0"
    );
    trial.fails_because("", "no unittest summary").await;
}

#[tokio::test]
async fn exiting_early_from_node_under_test_fails() {
    let (trial, answer) = Trial::solved("multi-repo-pagination");
    trial.prepend("inventory-client", "client.js", "process.exit(0);\n");
    let client = trial.repo("inventory-client");
    assert!(exits_zero(&client, "node", &["--test"]));
    trial
        .fails_because(&answer, "tests passed, expected at least 7")
        .await;
}

#[tokio::test]
async fn switching_cargo_tests_off_fails() {
    let (trial, answer) = Trial::solved("refactor-order-status");
    let manifest = trial.repo("refactor-order-status").join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let text = text.replace(
        "edition = \"2021\"",
        "edition = \"2021\"\nautotests = false",
    );
    assert!(
        text.contains("autotests = false"),
        "the manifest changed shape"
    );
    std::fs::write(&manifest, text).unwrap();
    trial
        .fails_because(&answer, "tests passed, expected at least 8")
        .await;
}

#[tokio::test]
async fn edits_to_visible_tests_never_count() {
    let (trial, answer) = Trial::solved("fix-off-by-one");
    let tests = trial.repo("fix-off-by-one").join("test_window.py");
    std::fs::write(&tests, "import unittest\n\nclass T(unittest.TestCase):\n    def test_x(self):\n        self.fail('agent edit')\n").unwrap();
    trial.commit_and_retag("fix-off-by-one");
    assert!(
        trial.grade(&answer).await.success,
        "the grader restores the task's own visible tests"
    );

    let lazy = Trial::new("fix-off-by-one");
    let tests = lazy.repo("fix-off-by-one").join("test_window.py");
    std::fs::write(&tests, "import unittest\n").unwrap();
    lazy.commit_and_retag("fix-off-by-one");
    lazy.fails_because("", "FAILED").await;
}

#[tokio::test]
async fn echoing_the_prompt_scores_nothing() {
    for id in ["review-seeded-bug", "ask-cache-invalidation"] {
        let trial = Trial::new(id);
        trial.fails_because(&trial.task.file.prompt, "MISS").await;
    }
}

#[tokio::test]
async fn keywords_scattered_through_a_long_answer_fail() {
    let filler = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12);
    let terms = [
        "The verify function is exported.",
        "It calls startsWith somewhere.",
        "An attacker could forge things.",
        "There is an authentication bypass.",
        "A prefix is involved.",
    ];
    let answer = terms.join(&format!("\n\n{filler}\n\n"));
    assert!(answer.len() < 16_000, "stays under the length cap");
    let trial = Trial::new("review-seeded-bug");
    trial.fails_because(&answer, "MISS").await;
}

#[tokio::test]
async fn padding_a_correct_answer_fails() {
    for id in ["review-seeded-bug", "ask-cache-invalidation"] {
        let (trial, answer) = Trial::solved(id);
        let padded = format!("{answer}\n{}", "keyword ".repeat(2_500));
        trial.fails_because(&padded, "TOO LONG").await;
    }
}
