//! What another process does between this module's steps is never lost: each
//! test makes that change at the exact point a race would.

use std::os::unix::fs::symlink;

use super::*;

const OPERATORS: &str = "language = \"en\"\n";
const OWN: &str = "whisper = \"/set/up/in/archon-tui\"\n";

struct Dirs {
    _root: tempfile::TempDir,
    source: PathBuf,
    instance: PathBuf,
    dest: PathBuf,
    scratch: PathBuf,
}

fn dirs() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let instance = path.join("instance");
    let scratch = instance.join("scratch");
    std::fs::create_dir_all(path.join("operator")).unwrap();
    std::fs::create_dir_all(&scratch).unwrap();
    let source = path.join("operator").join(FILE);
    std::fs::write(&source, OPERATORS).unwrap();
    Dirs {
        _root: root,
        source,
        dest: instance.join(FILE),
        instance,
        scratch,
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn is_operators(dirs: &Dirs) -> impl FnOnce(&Path) -> bool + '_ {
    |target| same_file(&dirs.instance.join(target), &dirs.source)
}

#[test]
fn a_file_created_while_the_copy_was_staged_is_not_replaced() {
    let dirs = dirs();
    let staged = stage(&dirs.source, &dirs.scratch).unwrap();
    std::fs::write(&dirs.dest, OWN).unwrap();
    let modified = std::fs::metadata(&dirs.dest).unwrap().modified().unwrap();

    assert!(!publish(&staged, &dirs.dest).unwrap());
    assert_eq!(std::fs::read_to_string(&dirs.dest).unwrap(), OWN);
    assert_eq!(
        std::fs::metadata(&dirs.dest).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(std::fs::read_to_string(&dirs.source).unwrap(), OPERATORS);
}

#[test]
fn a_link_created_while_the_copy_was_staged_is_not_replaced_or_followed() {
    let dirs = dirs();
    let staged = stage(&dirs.source, &dirs.scratch).unwrap();
    symlink(&dirs.source, &dirs.dest).unwrap();

    assert!(!publish(&staged, &dirs.dest).unwrap());
    assert_eq!(std::fs::read_link(&dirs.dest).unwrap(), dirs.source);
    assert_eq!(std::fs::read_to_string(&dirs.source).unwrap(), OPERATORS);
}

#[test]
fn a_link_that_became_an_own_file_is_put_back_untouched() {
    let dirs = dirs();
    // Inspected as the operator's link, then replaced by `/voice setup` or an editor.
    std::fs::write(&dirs.dest, OWN).unwrap();
    let modified = std::fs::metadata(&dirs.dest).unwrap().modified().unwrap();
    let held = dirs.scratch.join(HELD);

    assert!(!take_link(&dirs.dest, &held, is_operators(&dirs)).unwrap());
    let meta = std::fs::symlink_metadata(&dirs.dest).unwrap();
    assert!(meta.is_file());
    assert_eq!(meta.modified().unwrap(), modified);
    assert_eq!(std::fs::read_to_string(&dirs.dest).unwrap(), OWN);
    assert!(std::fs::symlink_metadata(&held).is_err());

    assert!(!replace_link(&dirs.source, &dirs.dest).unwrap());
    assert_eq!(std::fs::read_to_string(&dirs.dest).unwrap(), OWN);
    assert_eq!(names(&dirs.instance), ["scratch", FILE]);
}

#[test]
fn a_link_that_now_points_elsewhere_or_a_directory_is_put_back() {
    let dirs = dirs();
    let other = dirs.instance.join("other.toml");
    std::fs::write(&other, OWN).unwrap();
    symlink("other.toml", &dirs.dest).unwrap();
    assert!(!replace_link(&dirs.source, &dirs.dest).unwrap());
    assert!(!remove_dangling(&dirs.dest).unwrap());
    assert_eq!(
        std::fs::read_link(&dirs.dest).unwrap(),
        Path::new("other.toml")
    );
    assert_eq!(std::fs::read_to_string(&other).unwrap(), OWN);

    std::fs::remove_file(&dirs.dest).unwrap();
    std::fs::create_dir(&dirs.dest).unwrap();
    std::fs::write(dirs.dest.join("kept"), "x").unwrap();
    assert!(!replace_link(&dirs.source, &dirs.dest).unwrap());
    assert!(!remove_dangling(&dirs.dest).unwrap());
    assert_eq!(
        std::fs::read_to_string(dirs.dest.join("kept")).unwrap(),
        "x"
    );
    assert_eq!(names(&dirs.instance), ["other.toml", "scratch", FILE]);
}

#[test]
fn a_file_created_after_the_link_was_taken_wins_over_the_copy() {
    let dirs = dirs();
    symlink(&dirs.source, &dirs.dest).unwrap();
    let staged = stage(&dirs.source, &dirs.scratch).unwrap();
    let held = dirs.scratch.join(HELD);
    assert!(take_link(&dirs.dest, &held, is_operators(&dirs)).unwrap());
    assert!(std::fs::symlink_metadata(&dirs.dest).is_err());
    std::fs::write(&dirs.dest, OWN).unwrap();

    assert!(!publish(&staged, &dirs.dest).unwrap());
    assert_eq!(std::fs::read_to_string(&dirs.dest).unwrap(), OWN);
}

#[test]
fn a_vanished_destination_is_not_an_error_and_nothing_is_created() {
    let dirs = dirs();
    assert!(!replace_link(&dirs.source, &dirs.dest).unwrap());
    assert!(!remove_dangling(&dirs.dest).unwrap());
    assert_eq!(names(&dirs.instance), ["scratch"]);
}

#[test]
fn a_failed_copy_leaves_the_link_and_no_scratch_directory() {
    let dirs = dirs();
    std::fs::remove_file(&dirs.source).unwrap();
    std::fs::create_dir(&dirs.source).unwrap();
    symlink(&dirs.source, &dirs.dest).unwrap();

    let error = replace_link(&dirs.source, &dirs.dest).unwrap_err();
    assert!(
        format!("{error:#}").contains("not a regular file"),
        "{error:#}"
    );
    assert_eq!(std::fs::read_link(&dirs.dest).unwrap(), dirs.source);
    assert_eq!(names(&dirs.instance), ["scratch", FILE]);
}

#[test]
fn a_directory_created_while_one_was_held_is_not_replaced() {
    use std::os::unix::fs::MetadataExt as _;

    let dirs = dirs();
    // Taken as the inspected link, but a directory by the time it was moved.
    std::fs::create_dir(&dirs.dest).unwrap();
    let taken = std::fs::metadata(&dirs.dest).unwrap().ino();
    let mut kept = None;
    let error = in_scratch(&dirs.instance, |scratch| {
        let held = scratch.join(HELD);
        std::fs::rename(&dirs.dest, &held).unwrap();
        std::fs::create_dir(&dirs.dest).unwrap();
        std::fs::set_permissions(&dirs.dest, std::fs::Permissions::from_mode(0o711)).unwrap();
        kept = Some(held.clone());
        put_back(&held, &dirs.dest, true)
    })
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("move it back by hand"), "{message}");
    assert!(message.contains("is kept, not removed"), "{message}");
    let theirs = std::fs::symlink_metadata(&dirs.dest).unwrap();
    assert!(theirs.is_dir());
    assert_ne!(theirs.ino(), taken, "the new directory is not replaced");
    assert_eq!(theirs.permissions().mode() & 0o777, 0o711);
    let held = std::fs::symlink_metadata(kept.unwrap()).unwrap();
    assert_eq!(held.ino(), taken, "the held directory is retained");
}

#[test]
fn a_copy_changed_right_after_it_was_published_is_not_checked_or_reset() {
    let dirs = dirs();
    let staged = stage(&dirs.source, &dirs.scratch).unwrap();
    let private = std::fs::symlink_metadata(&staged).unwrap();
    assert!(private.is_file());
    assert_eq!(private.permissions().mode() & 0o777, MODE);

    assert!(publish(&staged, &dirs.dest).unwrap());
    std::fs::set_permissions(&dirs.dest, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(!publish(&staged, &dirs.dest).unwrap());
    let meta = std::fs::symlink_metadata(&dirs.dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o640);
    assert_eq!(std::fs::read_to_string(&dirs.dest).unwrap(), OPERATORS);
}
