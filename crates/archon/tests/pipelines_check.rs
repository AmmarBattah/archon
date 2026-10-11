//! `archon pipelines check` loads the catalog locally, without a daemon.

use std::path::Path;
use std::process::{Command, Output};

fn archon(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_archon"))
        .args(args)
        .env("ARCHON_CONFIG_DIR", home.join("config"))
        .env("ARCHON_DATA_DIR", home.join("data"))
        // Nothing listens here, so nothing can reach a real daemon.
        .env("ARCHON_SOCKET", home.join("archond.sock"))
        .env("NO_COLOR", "1")
        .output()
        .expect("running the archon binary")
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_broken_user_pipeline_fails_the_check() {
    let home = tempfile::tempdir().unwrap();
    let ok = archon(home.path(), &["pipelines", "check"]);
    assert!(ok.status.success(), "built-ins alone are valid: {ok:?}");

    let broken = home.path().join("config/pipelines/broken/pipeline.toml");
    write(&broken, "this is = not [valid toml");
    let out = archon(home.path(), &["pipelines", "check"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "a broken pipeline must fail the check"
    );
    assert!(
        stdout.contains("broken broken (user)"),
        "names the problem: {stdout}"
    );
    assert!(stdout.contains("1 broken pipeline(s)"), "{stdout}");
    assert!(
        !home.path().join("archond.sock").exists(),
        "no daemon was started"
    );
}

#[test]
fn a_broken_repo_pipeline_fails_the_check_for_that_repo() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("repo");
    write(
        &repo.join(".archon/pipelines/wonky/pipeline.toml"),
        "[pipeline]\nname = 3\n",
    );
    let out = archon(
        home.path(),
        &["pipelines", "check", "--repo", repo.to_str().unwrap()],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("broken wonky (repo)"));

    let list = archon(
        home.path(),
        &[
            "--json",
            "pipelines",
            "list",
            "--repo",
            repo.to_str().unwrap(),
        ],
    );
    assert!(
        list.status.success(),
        "listing shows problems without failing"
    );
    let listing: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(listing["problems"].as_array().unwrap().len(), 1);
}
