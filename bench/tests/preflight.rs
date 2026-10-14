//! Fail before starting daemons or agents when the private environment is invalid.

use std::path::Path;
use std::process::Command;

fn bench(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_archon-bench"));
    command.env_clear().args(args);
    command
}

#[test]
fn plain_daemon_runs_and_bootstrap_require_an_explicit_socket() {
    for args in [
        vec!["run", "--arms", "archon"],
        vec!["run", "--arms", "aether"],
        vec!["smoke", "--bootstrap"],
    ] {
        let output = bench(&args).output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("need --aether-socket")
        );
    }
}

fn sandbox_command(data: &Path) -> Command {
    assert!(data.is_absolute());
    assert!(data.is_dir());
    let root = data.join("archon-bench");
    std::fs::create_dir(&root).unwrap();
    let mut command = bench(&["run", "--arms", "archon,aether"]);
    command.env("XDG_DATA_HOME", data);
    command.env("ARCHON_SOCKET", root.join("archon.sock"));
    command.arg("--aether-socket").arg(root.join("aether.sock"));
    for name in [
        "ARCHON_CONFIG_DIR",
        "ARCHON_DATA_DIR",
        "AETHER_CONFIG_DIR",
        "AETHER_DATA_DIR",
    ] {
        let path = root.join(name);
        std::fs::create_dir(&path).unwrap();
        command.env(name, path);
    }
    command
}

#[test]
fn every_private_directory_is_required_and_aliases_fail_closed() {
    let data = tempfile::tempdir().unwrap();
    let mut command = sandbox_command(data.path());
    let root = data.path().join("archon-bench");
    command.env_remove("ARCHON_DATA_DIR");
    let missing = command.output().unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("need ARCHON_DATA_DIR")
    );
    for alias in [root.join("ARCHON_CONFIG_DIR"), root.join("alias")] {
        if alias.file_name().unwrap() == "alias" {
            std::os::unix::fs::symlink(root.join("ARCHON_CONFIG_DIR"), &alias).unwrap();
        }
        command.env("ARCHON_DATA_DIR", alias);
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("must be distinct")
        );
    }
}

#[test]
fn bootstrap_and_check_results_cannot_be_combined() {
    let output = bench(&["smoke", "--bootstrap", "--check-results", "unused"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot be used with")
    );
}
