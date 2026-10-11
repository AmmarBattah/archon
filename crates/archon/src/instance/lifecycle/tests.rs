use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread::JoinHandle;

use super::*;

/// A fake `aether` that logs its verb and environment, one line per call.
struct Shim {
    _root: tempfile::TempDir,
    program: PathBuf,
    log: PathBuf,
    instance: Instance,
}

fn shim(body: &str) -> Shim {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let log = base.join("calls.log");
    let program = base.join("aether");
    let script = format!(
        "#!/bin/sh\nprintf '%s|%s|%s|%s|%s|%s|%s|%s\\n' \"$*\" \"$AETHER_CONFIG_DIR\" \
         \"$AETHER_DATA_DIR\" \"$AETHER_SOCKET\" \"$ARCHON_CONFIG_DIR\" \"$ARCHON_DATA_DIR\" \
         \"$ARCHON_SOCKET\" \"$(pwd -P)\" >> '{}'\n{body}\n",
        log.display()
    );
    archon_core::testing::write_script(&program, &script);
    let instance = Instance::from_archon_dirs(
        base.join("config"),
        base.join("data"),
        base.join("run").join("archond.sock"),
    );
    std::fs::create_dir_all(&instance.config_dir).unwrap();
    std::fs::write(instance.config_path(), "").unwrap();
    Shim {
        _root: root,
        program,
        log,
        instance,
    }
}

impl Shim {
    fn calls(&self) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        text.lines()
            .map(|line| line.split('|').map(str::to_owned).collect())
            .collect()
    }
}

/// Stands in for the daemon the shim "starts": listens once the log shows
/// `daemon`, and goes away once it shows `stop`.
fn fake_aetherd(log: PathBuf, socket: PathBuf) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut listener = None;
        while Instant::now() < deadline {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            if listener.is_none() && text.lines().any(|l| l.starts_with("daemon|")) {
                std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
                listener = Some(UnixListener::bind(&socket).unwrap());
            }
            if text.lines().any(|l| l.starts_with("stop|")) {
                drop(listener);
                std::fs::remove_file(&socket).unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })
}

#[test]
fn start_and_stop_call_aether_with_the_instance_environment() {
    let shim = shim("sleep 0.2");
    let daemon = fake_aetherd(shim.log.clone(), shim.instance.socket.clone());
    let started = ensure_running(&shim.instance, &shim.program).unwrap();
    assert_eq!(started, Start::Started);
    assert!(is_listening(&shim.instance.socket));
    assert_eq!(
        ensure_running(&shim.instance, &shim.program).unwrap(),
        Start::AlreadyRunning
    );
    assert_eq!(stop(&shim.instance, &shim.program).unwrap(), Stop::Stopped);
    assert_eq!(
        stop(&shim.instance, &shim.program).unwrap(),
        Stop::NotRunning
    );
    daemon.join().unwrap();

    let calls = shim.calls();
    let verbs: Vec<_> = calls.iter().map(|c| c[0].as_str()).collect();
    assert_eq!(verbs, ["daemon", "stop"], "one start, one stop");
    let i = &shim.instance;
    for call in &calls {
        let expected = [
            &i.config_dir,
            &i.data_dir,
            &i.socket,
            &i.archon_config_dir,
            &i.archon_data_dir,
            &i.archon_socket,
            &i.data_dir,
        ];
        let got: Vec<_> = call[1..].iter().map(PathBuf::from).collect();
        let expected: Vec<_> = expected.into_iter().cloned().collect();
        assert_eq!(got, expected, "env and working directory of {:?}", call[0]);
    }
}

#[test]
fn concurrent_starts_launch_one_daemon() {
    let shim = shim("sleep 0.3");
    let daemon = fake_aetherd(shim.log.clone(), shim.instance.socket.clone());
    let results: Vec<Start> = std::thread::scope(|scope| {
        let starts: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| ensure_running(&shim.instance, &shim.program).unwrap()))
            .collect();
        starts.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(shim.calls().len(), 1, "the lock let one start through");
    let started = results.iter().filter(|r| **r == Start::Started).count();
    assert_eq!(started, 1, "{results:?}");
    stop(&shim.instance, &shim.program).unwrap();
    daemon.join().unwrap();
}

#[test]
fn a_failed_start_carries_aethers_words() {
    let shim = shim("echo 'no providers configured' >&2; exit 2");
    let error = ensure_running(&shim.instance, &shim.program).unwrap_err();
    let text = format!("{error:#}");
    assert!(text.contains("no providers configured"), "{text}");
    assert!(text.contains("exit status: 2"), "{text}");
    assert!(!is_listening(&shim.instance.socket));
}

#[test]
fn an_instance_without_setup_is_never_started() {
    let shim = shim("exit 0");
    std::fs::remove_file(shim.instance.config_path()).unwrap();
    let error = ensure_running(&shim.instance, &shim.program).unwrap_err();
    assert!(error.to_string().contains("archon setup"), "{error}");
    assert!(shim.calls().is_empty(), "aether was not run");
}
