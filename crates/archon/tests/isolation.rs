//! One archond per data directory, reads that never start one, and no
//! autostart where the config dir, data dir and socket mix moved and default
//! locations.

mod common;

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{Env, start_daemon, stop_daemon};

const NOT_RUNNING: &str = "archond is not running (start it with `archon daemon start`)";

/// The output of `child`, which must exit within `limit`; killed otherwise.
fn exit_within(mut child: Child, limit: Duration) -> Output {
    let deadline = Instant::now() + limit;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _already_gone = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!("still running after {limit:?}: {out:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_second_archond_on_the_same_data_dir_is_refused_by_name() {
    let env = Env::new();
    let first = start_daemon(&env);
    let second = env
        .command(&["daemon", "run"])
        .env("ARCHON_SOCKET", env.path("second.sock"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = exit_within(second, common::START_WAIT);
    let stderr = text(&out.stderr);
    assert!(!out.status.success(), "the second daemon ran: {out:?}");
    let holder = format!(
        "another archond (pid {}, socket {})",
        first.0.id(),
        env.path("archond.sock").display()
    );
    assert!(stderr.contains(&holder), "{stderr}");
    assert!(
        stderr.contains(&env.path("data").display().to_string()),
        "{stderr}"
    );
    assert!(!env.path("second.sock").exists(), "it never listened");
    env.run(&["daemon", "status"]);
    stop_daemon(&env, first);
}

#[test]
fn reads_with_no_daemon_start_nothing() {
    let env = Env::new();
    let reads: [&[&str]; 14] = [
        &["queue", "list"],
        &["queue", "show", "qi_x"],
        &["queue", "caps"],
        &["status"],
        &["--json", "status"],
        &["questions"],
        &["log"],
        &["cost"],
        &["reports"],
        &["repo", "list"],
        &["autonomy"],
        &["initiatives"],
        &["orchestrator", "status"],
        &["github-watcher"],
    ];
    for args in reads {
        let out = env.command(args).output().unwrap();
        // Under --json the error is printed on stdout.
        let said = text(&out.stderr) + &text(&out.stdout);
        assert!(!out.status.success(), "archon {args:?} succeeded: {out:?}");
        assert!(said.contains(NOT_RUNNING), "archon {args:?}: {said}");
        assert!(!said.contains("starting it"), "archon {args:?}: {said}");
    }
    let lessons = env.run(&["lessons"]);
    assert!(text(&lessons.stderr).is_empty(), "{lessons:?}");
    assert_nothing_started(&env.path("archond.sock"), &env.path("data"));
}

fn assert_nothing_started(socket: &Path, data: &Path) {
    assert!(!socket.exists(), "a socket at {}", socket.display());
    assert!(
        !socket.with_extension("log").exists(),
        "a detached start's log beside {}",
        socket.display()
    );
    assert!(
        !data.join("archon.db").exists(),
        "a store in {}",
        data.display()
    );
}

/// `env`'s commands with the defaults moved into its temp dir, and only
/// `ARCHON_CONFIG_DIR` and `ARCHON_SOCKET` set: the data dir is the default.
fn mixed(env: &Env, args: &[&str]) -> Command {
    let mut command = env.command(args);
    command
        .env_remove("ARCHON_DATA_DIR")
        .env("HOME", env.path("home"))
        .env("XDG_CONFIG_HOME", env.path("home/.config"))
        .env("XDG_DATA_HOME", env.path("home/.local/share"))
        .env("XDG_RUNTIME_DIR", env.path("run"));
    command
}

#[test]
fn a_mixed_environment_warns_and_starts_nothing() {
    let env = Env::new();
    let default_data = env.path("home/.local/share/archon");
    for args in [&["say", "hi"][..], &["daemon", "start"], &["repo", "list"]] {
        let out = mixed(&env, args).output().unwrap();
        let stderr = text(&out.stderr);
        assert!(!out.status.success(), "archon {args:?} succeeded: {out:?}");
        let kept = format!(
            "ARCHON_DATA_DIR ({}) is still the default",
            default_data.display()
        );
        assert!(stderr.contains(&kept), "archon {args:?}: {stderr}");
        assert!(
            stderr.contains("ARCHON_SOCKET ("),
            "archon {args:?}: {stderr}"
        );
    }
    let refused = mixed(&env, &["say", "hi"]).output().unwrap();
    assert!(
        text(&refused.stderr).contains("it is not started for you"),
        "{refused:?}"
    );
    let read = mixed(&env, &["queue", "list"]).output().unwrap();
    assert!(
        text(&read.stderr).contains(&format!("{NOT_RUNNING}. Note: ")),
        "{read:?}"
    );

    let doctor = mixed(&env, &["--json", "doctor"]).output().unwrap();
    let checks: Vec<serde_json::Value> = serde_json::from_slice(&doctor.stdout).unwrap();
    let locations = checks.iter().find(|c| c["check"] == "locations").unwrap();
    assert_eq!(locations["level"], "warn", "{checks:#?}");
    assert_nothing_started(&env.path("archond.sock"), &default_data);
    assert!(
        !env.path("run/archon").exists(),
        "nothing at the default socket"
    );
}

/// Aether spawns `archon mcp` with its own environment, which may set only
/// `ARCHON_SOCKET`: the proxy then connects but never starts archond.
#[test]
fn the_mcp_proxy_does_not_start_archond_in_a_mixed_environment() {
    let env = Env::new();
    let mut proxy = mixed(&env, &["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_repos","arguments":{}}}"#;
    let mut stdin = proxy.stdin.take().unwrap();
    std::io::Write::write_all(&mut stdin, format!("{call}\n").as_bytes()).unwrap();
    drop(stdin);
    let out = exit_within(proxy, common::START_WAIT);
    let reply: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let said = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(said.contains("it is not started for you"), "{said}");
    assert!(said.contains("ARCHON_DATA_DIR ("), "{said}");
    assert_nothing_started(
        &env.path("archond.sock"),
        &env.path("home/.local/share/archon"),
    );
}

#[test]
fn a_write_with_every_location_moved_starts_archond_and_says_so() {
    let env = Env::new();
    let paused = env.command(&["pause"]).output().unwrap();
    // Stopped before any assertion, so a failed one leaves no daemon behind.
    let stopped = env.command(&["daemon", "stop"]).output().unwrap();
    assert!(paused.status.success(), "{paused:?}");
    assert!(stopped.status.success(), "{stopped:?}");
    let stderr = text(&paused.stderr);
    assert!(
        stderr.contains("archond is not running; starting it"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&env.path("data").display().to_string()),
        "{stderr}"
    );
}
