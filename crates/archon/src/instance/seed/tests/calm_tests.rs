use super::*;
use crate::instance::seed::calm::{self, State};

#[test]
fn calm_is_seeded_once_and_resync_preserves_operator_edits() {
    let dirs = dirs();
    let bin = Path::new("/opt/archon");
    assert_eq!(
        calm::inspect(&dirs.instance.config_dir).unwrap(),
        State::Missing
    );
    let first = seed(&dirs.instance, &dirs.operator, bin, false).unwrap();
    assert_eq!(first.calm, "seeded");
    let path = dirs.instance.config_dir.join(calm::FILE);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), calm::DEFINITION);
    let again = seed(&dirs.instance, &dirs.operator, bin, false).unwrap();
    assert_eq!(again.calm, State::Builtin.description());
    let edited = calm::DEFINITION.replace("compact", "comfortable");
    std::fs::write(&path, &edited).unwrap();
    for resync in [false, true] {
        let outcome = seed(&dirs.instance, &dirs.operator, bin, resync).unwrap();
        assert_eq!(outcome.calm, State::Edited.description());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    }
    assert!(!dirs.operator.join(calm::FILE).exists());
}

#[test]
fn calm_definition_scopes_coordination_to_the_orchestrator() {
    let table: Table = toml::from_str(calm::DEFINITION).unwrap();
    assert_eq!(table["version"].as_integer(), Some(1));
    assert_eq!(table["density"].as_str(), Some("compact"));
    assert_eq!(table["scope"]["role"].as_str(), Some("archon-orchestrator"));
    let shown: Vec<_> = table["show"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        shown,
        [
            "user_text",
            "assistant_text",
            "delegation",
            "agent_message",
            "completion",
            "status",
            "question",
            "approval",
            "error"
        ]
    );
}

#[test]
fn calm_does_not_replace_links_or_directories() {
    let dirs = dirs();
    let config = &dirs.instance.config_dir;
    std::fs::create_dir_all(config.join("modes")).unwrap();
    let path = config.join(calm::FILE);
    std::os::unix::fs::symlink("/missing/calm.toml", &path).unwrap();
    assert_eq!(calm::seed(config).unwrap(), State::NotRegular.description());
    assert_eq!(
        std::fs::read_link(&path).unwrap(),
        Path::new("/missing/calm.toml")
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(calm::seed(config).unwrap(), State::NotRegular.description());
    assert!(path.is_dir());
}

#[test]
fn calm_never_seeds_through_a_linked_modes_directory() {
    let dirs = dirs();
    std::fs::create_dir_all(&dirs.instance.config_dir).unwrap();
    std::os::unix::fs::symlink(&dirs.operator, dirs.instance.config_dir.join("modes")).unwrap();
    assert_eq!(
        calm::seed(&dirs.instance.config_dir).unwrap(),
        State::NotRegular.description()
    );
    assert!(!dirs.operator.join("calm.toml").exists());
}

#[test]
fn calm_reports_a_read_failure_without_replacing_the_file() {
    let dirs = dirs();
    let path = dirs.instance.config_dir.join(calm::FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, [0xff]).unwrap();
    assert!(
        calm::seed(&dirs.instance.config_dir)
            .unwrap_err()
            .to_string()
            .contains("reading")
    );
    assert_eq!(std::fs::read(&path).unwrap(), [0xff]);
}

#[test]
fn calm_reports_a_catalog_aether_cannot_load() {
    let dirs = dirs();
    let config = &dirs.instance.config_dir;
    calm::seed(config).unwrap();
    for n in 0..128 {
        std::fs::write(
            config.join(format!("modes/other{n}.toml")),
            "version = 1\nshow = []",
        )
        .unwrap();
    }
    let error = calm::inspect(config).unwrap_err();
    assert!(error.to_string().contains("more than 128"), "{error}");
}
