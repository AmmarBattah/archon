//! `Lavish` against the fake lavish-axi in `tests/fixtures`, symlinked into a
//! scratch dir that holds its canned replies.

use std::path::{Path, PathBuf};
use std::time::Duration;

use archon_core::config::{LavishConfig, Toggle};
use archon_core::error::ArchonError;
use archon_lavish::{Lavish, LavishFeedback};

struct FakeAxi {
    dir: tempfile::TempDir,
    lavish: Lavish,
    html: PathBuf,
}

impl FakeAxi {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lavish-axi");
        let binary = dir.path().join("lavish-axi");
        std::os::unix::fs::symlink(fixture, &binary).unwrap();
        let html = dir.path().join("report.html");
        std::fs::write(&html, "<h1>report</h1>").unwrap();
        let lavish = Lavish::detect(&LavishConfig {
            enabled: Toggle::On,
            binary,
        })
        .unwrap()
        .unwrap();
        Self { dir, lavish, html }
    }

    /// Written then renamed so a waiting poll never sees half a reply.
    fn reply(&self, name: &str, body: &str) {
        let staged = self.dir.path().join(format!(".{name}"));
        std::fs::write(&staged, body).unwrap();
        std::fs::rename(staged, self.dir.path().join(name)).unwrap();
    }

    fn fail(&self, name: &str, body: &str, code: i32) {
        std::fs::write(
            self.dir.path().join(format!("{name}.exit")),
            code.to_string(),
        )
        .unwrap();
        self.reply(name, body);
    }

    fn calls(&self) -> Vec<String> {
        let calls = std::fs::read_to_string(self.dir.path().join("calls")).unwrap_or_default();
        calls.lines().map(str::to_string).collect()
    }

    fn file(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.path().join(name)).ok()
    }
}

fn feedback(extra_session: &str, rest: &str) -> String {
    format!(
        "session:\n  file: /r.html\n  status: feedback\n{extra_session}{rest}next_step: Apply\ndom_snapshot: \"\"\n"
    )
}

#[tokio::test]
async fn open_returns_the_session_url() {
    let fake = FakeAxi::new();
    fake.reply(
        "open",
        "session:\n  file: /r.html\n  url: \"http://127.0.0.1:4455/session/abc\"\n  status: opened\nnext_step: poll\n",
    );
    let session = fake.lavish.open(&fake.html).await.unwrap();
    assert_eq!(session.url, "http://127.0.0.1:4455/session/abc");
    assert_eq!(session.html, fake.html);
    assert_eq!(fake.calls(), vec![fake.html.display().to_string()]);
}

#[tokio::test]
async fn open_refuses_a_session_the_operator_ended() {
    let fake = FakeAxi::new();
    fake.reply(
        "open",
        "session:\n  file: /r.html\n  url: \"http://127.0.0.1:1/s\"\n  status: user-ended\n",
    );
    let err = fake.lavish.open(&fake.html).await.unwrap_err();
    assert!(
        matches!(err, ArchonError::Unavailable(ref m) if m.contains("ended")),
        "{err}"
    );
}

/// An explicit reopen passes `--reopen`, which lavish-axi needs to
/// show a session the operator ended.
#[tokio::test]
async fn reopen_asks_lavish_to_reopen_an_ended_session() {
    let fake = FakeAxi::new();
    fake.reply(
        "open",
        "session:\n  file: /r.html\n  url: \"http://127.0.0.1:1/s\"\n  status: opened\n",
    );
    let session = fake.lavish.reopen(&fake.html).await.unwrap();
    assert_eq!(session.url, "http://127.0.0.1:1/s");
    let expected = format!("{} --reopen", fake.html.display());
    assert_eq!(fake.calls(), vec![expected]);
}

#[tokio::test]
async fn a_failing_command_reports_both_streams() {
    let fake = FakeAxi::new();
    fake.fail("open", "error: server down\ncode: SERVER_ERROR\n", 2);
    let err = fake.lavish.open(&fake.html).await.unwrap_err().to_string();
    assert!(err.contains("SERVER_ERROR"), "{err}");
    assert!(err.contains("exit status: 2"), "{err}");
}

#[tokio::test]
async fn poll_waits_through_timeouts_and_sends_the_reply_once() {
    let fake = FakeAxi::new();
    fake.reply("poll.1", "session:\n  file: /r.html\n  status: waiting\n");
    fake.reply(
        "poll.2",
        &feedback(
            "",
            "prompts[1]{uid,prompt,selector,tag,text}:\n  u1,\"Drop the chart, keep the table\",div,div,Chart\n",
        ),
    );
    let got = fake
        .lavish
        .poll(&fake.html, Some("Updated the summary"))
        .await
        .unwrap();
    let LavishFeedback::Prompts { prompts, raw } = got else {
        panic!("expected prompts, got {got:?}");
    };
    assert_eq!(prompts, vec!["Drop the chart, keep the table"]);
    assert!(raw.contains("u1"));
    assert_eq!(fake.file("reply.1").as_deref(), Some("Updated the summary"));
    assert_eq!(fake.file("reply.2"), None, "the reply must not be re-sent");
    let html = fake.html.display();
    assert_eq!(
        fake.calls(),
        vec![
            format!("poll {html} --agent-reply-file -"),
            format!("poll {html}")
        ]
    );
}

#[tokio::test]
async fn poll_blocks_until_feedback_arrives() {
    let fake = FakeAxi::new();
    let html = fake.html.clone();
    let lavish = fake.lavish.clone();
    let poll = tokio::spawn(async move { lavish.poll(&html, None).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!poll.is_finished(), "poll returned before any feedback");
    fake.reply("poll.1", &feedback("", "prompts[0]:\n"));
    let got = poll.await.unwrap().unwrap();
    assert!(matches!(got, LavishFeedback::Prompts { ref prompts, .. } if prompts.is_empty()));
}

#[tokio::test]
async fn send_and_end_is_ended_but_keeps_the_final_prompts() {
    let fake = FakeAxi::new();
    fake.reply(
        "poll",
        &feedback(
            "  session_ended: true\n  ended_by: user\n",
            "prompts[1]{uid,prompt,selector,tag,text}:\n  u9,Ship it,body,body,\n",
        ),
    );
    let got = fake.lavish.poll(&fake.html, None).await.unwrap();
    assert!(
        matches!(got, LavishFeedback::Ended { ref raw, ref prompts } if raw.contains("Ship it") && prompts == &["Ship it"]),
        "{got:?}"
    );
}

#[tokio::test]
async fn ended_disconnected_and_artifact_failures_are_distinguished() {
    let cases = [
        (
            "session:\n  file: /r.html\n  status: ended\n  ended_by: user\n".to_string(),
            "ended",
        ),
        (
            "session:\n  file: /r.html\n  status: browser_disconnected\n".to_string(),
            "disconnected",
        ),
        (
            feedback(
                "",
                "prompts[0]:\nartifact_failures[1]{kind,detail}:\n  asset,style.css missing\n",
            ),
            "artifact",
        ),
    ];
    for (reply, expected) in cases {
        let fake = FakeAxi::new();
        fake.reply("poll", &reply);
        let got = fake.lavish.poll(&fake.html, None).await.unwrap();
        let kind = match got {
            LavishFeedback::Ended { .. } => "ended",
            LavishFeedback::Disconnected { .. } => "disconnected",
            LavishFeedback::ArtifactFailure { .. } => "artifact",
            LavishFeedback::Prompts { .. } => "prompts",
        };
        assert_eq!(kind, expected, "for {reply}");
    }
}

#[tokio::test]
async fn unknown_output_is_a_protocol_error_that_keeps_the_text() {
    let fake = FakeAxi::new();
    fake.reply("poll", "something: else\n");
    let err = fake.lavish.poll(&fake.html, None).await.unwrap_err();
    assert!(
        matches!(err, ArchonError::Protocol(ref m) if m.contains("something: else")),
        "{err}"
    );
}

#[tokio::test]
async fn end_closes_the_session() {
    let fake = FakeAxi::new();
    fake.reply("end", "session:\n  file: /r.html\n  status: ended\n");
    fake.lavish.end(&fake.html).await.unwrap();
    assert_eq!(fake.calls(), vec![format!("end {}", fake.html.display())]);
}

#[tokio::test]
async fn a_vanished_binary_is_an_io_error_not_a_panic() {
    let fake = FakeAxi::new();
    std::fs::remove_file(fake.dir.path().join("lavish-axi")).unwrap();
    let err = fake.lavish.end(&fake.html).await.unwrap_err();
    assert!(matches!(err, ArchonError::Io { .. }), "{err}");
}

#[tokio::test]
async fn relative_paths_are_refused_before_running_anything() {
    let fake = FakeAxi::new();
    let err = fake
        .lavish
        .open(Path::new("report.html"))
        .await
        .unwrap_err();
    assert!(matches!(err, ArchonError::Invalid(_)), "{err}");
    assert!(fake.calls().is_empty());
}
