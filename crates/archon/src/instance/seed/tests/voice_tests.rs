//! The instance's `voice.toml`: its own regular file, never a link.

use std::os::unix::fs::{PermissionsExt as _, symlink};

use super::super::voice::{self, VoiceFile, VoiceOutcome};
use super::*;

const VOICE: &str = "whisper = \"/home/op/.config/aether/voice/whisper.cpp-1/whisper-cli\"\n";
const BIN: &str = "/opt/archon";
const LOCK: &str = ".voice-setup.lock";

/// Directories with an operator `voice.toml` and an existing instance directory.
fn voice_dirs() -> Dirs {
    let dirs = dirs();
    std::fs::write(dirs.operator.join(voice::FILE), VOICE).unwrap();
    std::fs::create_dir_all(&dirs.instance.config_dir).unwrap();
    dirs
}

fn dest(dirs: &Dirs) -> PathBuf {
    dirs.instance.config_dir.join(voice::FILE)
}

fn setup(dirs: &Dirs, resync: bool) -> VoiceOutcome {
    seed(&dirs.instance, &dirs.operator, Path::new(BIN), resync)
        .unwrap()
        .voice
}

/// What `archon tui` runs before it starts the TUI.
fn tui_hook(dirs: &Dirs) -> VoiceOutcome {
    voice::heal(&dirs.operator, &dirs.instance.config_dir).unwrap()
}

/// Aether's rule: `voice.toml` is a regular file. Here also private, with these bytes.
fn assert_own_copy(dirs: &Dirs, bytes: &str) {
    let meta = std::fs::symlink_metadata(dest(dirs)).unwrap();
    assert!(meta.is_file(), "a regular file, not a link: {meta:?}");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(dest(dirs)).unwrap(), bytes);
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_fresh_seed_copies_voice_toml_as_the_instances_own_private_file() {
    let dirs = dirs();
    let source = dirs.operator.join(voice::FILE);
    std::fs::write(&source, VOICE).unwrap();
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o644)).unwrap();
    let before = snapshot(&dirs.operator);

    assert_eq!(setup(&dirs, false), VoiceOutcome::Copied);
    assert_own_copy(&dirs, VOICE);
    assert!(std::fs::symlink_metadata(dest(&dirs)).unwrap().is_file());
    assert_eq!(
        names(&dirs.instance.config_dir),
        [".voice-setup.lock", "config.toml", "modes", "voice.toml"],
        "no temporary file is left"
    );

    let instance = snapshot(&dirs.instance.config_dir);
    let own = VoiceOutcome::Unchanged(VoiceFile::OwnFile);
    assert_eq!(setup(&dirs, false), own);
    assert_eq!(tui_hook(&dirs), own);
    assert_eq!(snapshot(&dirs.instance.config_dir), instance);
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn setup_heals_a_link_to_the_operators_voice_toml() {
    for resync in [false, true] {
        let dirs = voice_dirs();
        symlink(dirs.operator.join(voice::FILE), dest(&dirs)).unwrap();
        let before = snapshot(&dirs.operator);

        assert_eq!(setup(&dirs, resync), VoiceOutcome::Healed);
        assert_own_copy(&dirs, VOICE);
        let own = VoiceOutcome::Unchanged(VoiceFile::OwnFile);
        assert_eq!(setup(&dirs, resync), own, "resync {resync}");
        assert_own_copy(&dirs, VOICE);
        assert_eq!(snapshot(&dirs.operator), before);
    }
}

#[test]
fn the_tui_start_hook_heals_a_link_and_then_changes_nothing() {
    let dirs = voice_dirs();
    // Relative, as a hand-made link may be: it still resolves to the operator's file.
    symlink("../../operator-aether/voice.toml", dest(&dirs)).unwrap();
    assert_eq!(
        voice::inspect(&dirs.operator, &dirs.instance.config_dir).unwrap(),
        VoiceFile::OperatorLink
    );
    let before = snapshot(&dirs.operator);

    assert_eq!(tui_hook(&dirs), VoiceOutcome::Healed);
    assert_own_copy(&dirs, VOICE);
    assert_eq!(names(&dirs.instance.config_dir), [LOCK, "voice.toml"]);
    let modified = std::fs::metadata(dest(&dirs)).unwrap().modified().unwrap();
    assert_eq!(tui_hook(&dirs), VoiceOutcome::Unchanged(VoiceFile::OwnFile));
    assert_eq!(
        std::fs::metadata(dest(&dirs)).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(snapshot(&dirs.operator), before);
}

/// What the seed does once its copy has a public name, where an editor can reach it.
fn settle(dirs: &Dirs, outcome: VoiceOutcome) -> VoiceOutcome {
    voice::settled(outcome, &dirs.operator, &dirs.instance.config_dir).unwrap()
}

#[test]
fn a_mode_change_right_after_the_copy_is_published_is_kept() {
    let dirs = dirs();
    std::fs::write(dirs.operator.join(voice::FILE), VOICE).unwrap();
    assert_eq!(setup(&dirs, false), VoiceOutcome::Copied);
    std::fs::set_permissions(dest(&dirs), std::fs::Permissions::from_mode(0o640)).unwrap();

    assert_eq!(settle(&dirs, VoiceOutcome::Copied), VoiceOutcome::Copied);
    let meta = std::fs::symlink_metadata(dest(&dirs)).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o640, "not reset");
    assert_eq!(std::fs::read_to_string(dest(&dirs)).unwrap(), VOICE);
}

#[test]
fn a_replacement_right_after_the_copy_is_published_is_reported_and_left() {
    let dirs = voice_dirs();
    symlink(dirs.operator.join(voice::FILE), dest(&dirs)).unwrap();
    assert_eq!(setup(&dirs, false), VoiceOutcome::Healed);
    let own = "whisper = \"/set/up/in/archon-tui\"\n";
    std::fs::write(dest(&dirs), own).unwrap();
    assert_eq!(settle(&dirs, VoiceOutcome::Healed), VoiceOutcome::Healed);
    assert_eq!(std::fs::read_to_string(dest(&dirs)).unwrap(), own);

    std::fs::remove_file(dest(&dirs)).unwrap();
    std::fs::create_dir(dest(&dirs)).unwrap();
    let before = snapshot(&dirs.operator);
    assert_eq!(
        settle(&dirs, VoiceOutcome::Healed),
        VoiceOutcome::Unchanged(VoiceFile::NotAFile)
    );
    assert!(std::fs::symlink_metadata(dest(&dirs)).unwrap().is_dir());
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn an_own_voice_toml_is_never_replaced_even_by_resync() {
    let dirs = voice_dirs();
    let own = "whisper = \"/set/up/in/archon-tui\"\n";
    std::fs::write(dest(&dirs), own).unwrap();
    std::fs::set_permissions(dest(&dirs), std::fs::Permissions::from_mode(0o640)).unwrap();
    let meta = std::fs::metadata(dest(&dirs)).unwrap();
    let before = snapshot(&dirs.operator);

    let left = VoiceOutcome::Unchanged(VoiceFile::OwnFile);
    assert_eq!(setup(&dirs, false), left);
    assert_eq!(setup(&dirs, true), left);
    assert_eq!(tui_hook(&dirs), left);
    let after = std::fs::metadata(dest(&dirs)).unwrap();
    assert_eq!(std::fs::read_to_string(dest(&dirs)).unwrap(), own);
    assert_eq!(after.modified().unwrap(), meta.modified().unwrap());
    assert_eq!(after.permissions(), meta.permissions());
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn a_dangling_link_is_removed_so_voice_setup_can_create_the_file() {
    let dirs = dirs();
    std::fs::create_dir_all(&dirs.instance.config_dir).unwrap();
    let gone = dirs.operator.join(voice::FILE);
    symlink(&gone, dest(&dirs)).unwrap();
    let before = snapshot(&dirs.operator);

    let removed = VoiceOutcome::DanglingRemoved(gone.clone());
    assert_eq!(tui_hook(&dirs), removed);
    assert!(std::fs::symlink_metadata(dest(&dirs)).is_err());
    let absent = VoiceOutcome::Unchanged(VoiceFile::Absent);
    assert_eq!(tui_hook(&dirs), absent);

    symlink(&gone, dest(&dirs)).unwrap();
    assert_eq!(setup(&dirs, false), removed);
    assert!(std::fs::symlink_metadata(dest(&dirs)).is_err());
    assert_eq!(setup(&dirs, true), absent);
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn setup_replaces_a_dangling_link_elsewhere_by_the_operators_copy() {
    let dirs = voice_dirs();
    symlink("/nowhere/voice.toml", dest(&dirs)).unwrap();
    let before = snapshot(&dirs.operator);

    assert_eq!(setup(&dirs, false), VoiceOutcome::Healed);
    assert_own_copy(&dirs, VOICE);
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn a_link_elsewhere_is_reported_and_left() {
    let dirs = voice_dirs();
    let other = dirs.operator.parent().unwrap().join("other-voice.toml");
    std::fs::write(&other, "language = \"sv\"\n").unwrap();
    symlink(&other, dest(&dirs)).unwrap();
    let before = snapshot(&dirs.operator);

    let left = VoiceOutcome::Unchanged(VoiceFile::Elsewhere(other.clone()));
    assert_eq!(tui_hook(&dirs), left);
    assert_eq!(setup(&dirs, false), left);
    assert_eq!(setup(&dirs, true), left);
    assert_eq!(std::fs::read_link(dest(&dirs)).unwrap(), other);
    assert_eq!(
        std::fs::read_to_string(&other).unwrap(),
        "language = \"sv\"\n"
    );
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn a_directory_is_reported_and_never_removed() {
    let dirs = voice_dirs();
    std::fs::create_dir(dest(&dirs)).unwrap();
    std::fs::write(dest(&dirs).join("kept"), "x").unwrap();
    let before = snapshot(&dirs.operator);

    let left = VoiceOutcome::Unchanged(VoiceFile::NotAFile);
    assert_eq!(tui_hook(&dirs), left);
    assert_eq!(setup(&dirs, false), left);
    assert_eq!(setup(&dirs, true), left);
    assert_eq!(
        std::fs::read_to_string(dest(&dirs).join("kept")).unwrap(),
        "x"
    );
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn without_an_operator_voice_toml_nothing_is_created() {
    let dirs = dirs();
    let before = snapshot(&dirs.operator);

    let absent = VoiceOutcome::Unchanged(VoiceFile::Absent);
    assert_eq!(setup(&dirs, false), absent);
    assert_eq!(setup(&dirs, true), absent);
    assert_eq!(tui_hook(&dirs), absent);
    assert!(std::fs::symlink_metadata(dest(&dirs)).is_err());
    assert_eq!(snapshot(&dirs.operator), before);
    assert!(!dirs.operator.join(voice::FILE).exists());
}

#[test]
fn the_hook_never_copies_where_the_instance_has_no_voice_toml() {
    let dirs = voice_dirs();
    assert_eq!(tui_hook(&dirs), VoiceOutcome::Unchanged(VoiceFile::Absent));
    assert!(std::fs::symlink_metadata(dest(&dirs)).is_err());
}

#[test]
fn an_operator_voice_toml_that_cannot_be_copied_fails_and_keeps_the_link() {
    let dirs = dirs();
    std::fs::create_dir_all(&dirs.instance.config_dir).unwrap();
    let source = dirs.operator.join(voice::FILE);
    std::fs::create_dir(&source).unwrap();
    symlink(&source, dest(&dirs)).unwrap();

    let error = voice::heal(&dirs.operator, &dirs.instance.config_dir).unwrap_err();
    assert!(
        format!("{error:#}").contains("not a regular file"),
        "{error:#}"
    );
    assert_eq!(std::fs::read_link(dest(&dirs)).unwrap(), source);
    assert_eq!(names(&dirs.instance.config_dir), [LOCK, "voice.toml"]);
}

/// Aether's installers hold `.voice-setup.lock` from before they read
/// `voice.toml` until they have saved it: nothing is changed under them.
#[test]
fn a_running_voice_setup_is_never_disturbed() {
    let dirs = voice_dirs();
    let source = dirs.operator.join(voice::FILE);
    let before = snapshot(&dirs.operator);
    let running = voice::lock_setup(&dirs.instance.config_dir)
        .unwrap()
        .expect("the lock is free");

    let absent = VoiceOutcome::SetupRunning(VoiceFile::Absent);
    assert_eq!(setup(&dirs, false), absent);
    assert_eq!(tui_hook(&dirs), VoiceOutcome::Unchanged(VoiceFile::Absent));
    assert!(std::fs::symlink_metadata(dest(&dirs)).is_err());

    symlink(&source, dest(&dirs)).unwrap();
    let linked = VoiceOutcome::SetupRunning(VoiceFile::OperatorLink);
    assert_eq!(setup(&dirs, true), linked);
    assert_eq!(tui_hook(&dirs), linked);
    assert_eq!(std::fs::read_link(dest(&dirs)).unwrap(), source);

    // What the running setup saves is the instance's own, lock held or not.
    std::fs::remove_file(dest(&dirs)).unwrap();
    std::fs::write(dest(&dirs), "language = \"sv\"\n").unwrap();
    let own = VoiceOutcome::Unchanged(VoiceFile::OwnFile);
    assert_eq!(setup(&dirs, true), own);
    running.unlock().unwrap();
    assert_eq!(setup(&dirs, true), own);
    assert_eq!(
        std::fs::read_to_string(dest(&dirs)).unwrap(),
        "language = \"sv\"\n"
    );
    assert_eq!(snapshot(&dirs.operator), before);
}

#[test]
fn the_setup_lock_is_released_and_never_followed_through_a_link() {
    let dirs = voice_dirs();
    symlink(dirs.operator.join(voice::FILE), dest(&dirs)).unwrap();
    assert_eq!(tui_hook(&dirs), VoiceOutcome::Healed);
    let free = voice::lock_setup(&dirs.instance.config_dir).unwrap();
    assert!(free.is_some(), "healing released the lock");
    drop(free);

    let lock = dirs.instance.config_dir.join(LOCK);
    std::fs::remove_file(&lock).unwrap();
    std::fs::remove_file(dest(&dirs)).unwrap();
    symlink(dirs.operator.join(LOCK), &lock).unwrap();
    let before = snapshot(&dirs.operator);
    let error = seed(&dirs.instance, &dirs.operator, Path::new(BIN), false).unwrap_err();
    assert!(format!("{error:#}").contains(LOCK), "{error:#}");
    assert!(std::fs::symlink_metadata(dest(&dirs)).is_err());
    assert_eq!(snapshot(&dirs.operator), before);
}

/// The first attempt's temporary name: a file that has it is not this call's.
#[test]
fn a_leftover_file_with_a_temporary_name_is_neither_used_nor_removed() {
    let dirs = voice_dirs();
    let name = format!(".voice.toml.{}.tmp", std::process::id());
    let leftover = dirs.instance.config_dir.join(&name);
    std::fs::write(&leftover, "not ours").unwrap();

    assert_eq!(setup(&dirs, false), VoiceOutcome::Copied);
    assert_own_copy(&dirs, VOICE);
    assert_eq!(std::fs::read_to_string(&leftover).unwrap(), "not ours");
    assert_eq!(
        names(&dirs.instance.config_dir),
        [LOCK, name.as_str(), "config.toml", "modes", "voice.toml"]
    );
}
