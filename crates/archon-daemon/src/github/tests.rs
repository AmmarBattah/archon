use archon_aether::github::{RepositoryHealth, WatcherConfig, WatcherStatus};
use archon_core::aether_ids::AetherAgentId;
use archon_core::error::ArchonError;
use archon_core::github::{
    AllowedUsers, GithubRepo, LABEL_REGEX, MENTION_REGEX, OperatorUsers, WatchTarget, WatcherChange,
};
use serde_json::json;

use super::{
    AETHER_LABEL_REGEX, AETHER_MENTION_REGEX, apply_change, check_operators_stored, delivering,
    newly_delivering, repoint, view,
};

const OLD: &str = "agt_01ARZ3NDEKTSV4RRFFQ69G5FAV";
const NEW: &str = "agt_01BX5ZZKBKACTAV9WEVGEMMVRZ";

fn agent(id: &str) -> AetherAgentId {
    AetherAgentId::parse(id).unwrap()
}

fn repo(name: &str) -> GithubRepo {
    GithubRepo::parse(name).unwrap()
}

/// Aether's shipped config, plus what a test lists.
fn config(repositories: serde_json::Value) -> WatcherConfig {
    serde_json::from_value(json!({
        "enabled": false,
        "mention_regex": AETHER_MENTION_REGEX,
        "label_regex": AETHER_LABEL_REGEX,
        "poll_interval_secs": 120,
        "repositories": repositories,
    }))
    .unwrap()
}

fn fixed(name: &str, target: &str) -> serde_json::Value {
    json!({"repository": name, "enabled": true, "agent": target, "dispatch": {"mode": "fixed"}})
}

fn thread(name: &str) -> serde_json::Value {
    json!({"repository": name, "enabled": true, "agent": null,
           "dispatch": {"mode": "thread", "role": "r", "objective": "o"}})
}

#[test]
fn adding_watches_through_the_orchestrator_with_archons_patterns() {
    let mut config = config(json!([]));
    let add = WatcherChange::Add {
        repository: repo("Owner/Repo"),
        allowed_users: None,
    };
    apply_change(&mut config, add, Some(&agent(NEW))).unwrap();
    assert!(config.enabled, "adding switches the watcher on");
    assert_eq!(config.mention_regex, MENTION_REGEX);
    assert_eq!(config.label_regex, LABEL_REGEX);
    let [added] = config.repositories.as_slice() else {
        panic!("one repository");
    };
    assert_eq!(added.repository, "owner/repo");
    assert!(added.enabled && added.is_fixed());
    assert_eq!(added.agent, Some(agent(NEW)));
}

#[test]
fn adding_keeps_tuned_patterns_and_refuses_another_dispatch_mode() {
    let mut config = config(json!([thread("o/thread")]));
    config.mention_regex = "(?i)@bot".into();
    let add = |name: &str| WatcherChange::Add {
        repository: repo(name),
        allowed_users: None,
    };
    apply_change(&mut config, add("o/new"), Some(&agent(NEW))).unwrap();
    assert_eq!(
        config.mention_regex, "(?i)@bot",
        "an operator's pattern stays"
    );
    let taken = apply_change(&mut config, add("O/Thread"), Some(&agent(NEW)));
    assert!(matches!(taken, Err(ArchonError::Invalid(_))), "{taken:?}");
    let no_target = apply_change(&mut config, add("o/other"), None);
    assert!(matches!(no_target, Err(ArchonError::Unavailable(_))));
    assert_eq!(config.repositories.len(), 2, "refusals change nothing");
}

#[test]
fn enable_disable_and_remove_name_a_watched_repository_or_the_watcher() {
    let mut config = config(json!([fixed("o/r", OLD)]));
    let target = agent(NEW);
    let disable = WatcherChange::Disable {
        repository: Some(repo("O/R")),
    };
    apply_change(&mut config, disable, None).unwrap();
    assert!(!config.repositories[0].enabled);
    let enable = WatcherChange::Enable {
        repository: Some(repo("o/r")),
    };
    apply_change(&mut config, enable, Some(&target)).unwrap();
    assert!(config.repositories[0].enabled);
    assert_eq!(config.repositories[0].agent, Some(target.clone()));
    apply_change(
        &mut config,
        WatcherChange::Enable { repository: None },
        None,
    )
    .unwrap();
    assert!(config.enabled);
    let missing = WatcherChange::Remove {
        repository: repo("o/missing"),
    };
    let err = apply_change(&mut config, missing, None).unwrap_err();
    assert!(matches!(err, ArchonError::NotFound(_)), "{err}");
    let remove = WatcherChange::Remove {
        repository: repo("o/r"),
    };
    apply_change(&mut config, remove, None).unwrap();
    assert!(config.repositories.is_empty());
}

fn users(logins: &[&str]) -> Option<AllowedUsers> {
    Some(AllowedUsers::parse(logins).unwrap())
}

#[test]
fn add_sets_the_allowlist_in_its_write_and_keeps_it_when_given_none() {
    let mut config = config(json!([]));
    let add = |allowed_users| WatcherChange::Add {
        repository: repo("Example-Org/example-repo"),
        allowed_users,
    };
    let team = users(&["monaLisa", "Hubot"]);
    apply_change(&mut config, add(team), Some(&agent(NEW))).unwrap();
    let expected = vec!["monalisa".to_owned(), "hubot".to_owned()];
    assert_eq!(config.repositories[0].allowed_users, Some(expected.clone()));
    assert_eq!(config.repositories[0].agent, Some(agent(NEW)));
    apply_change(&mut config, add(None), Some(&agent(NEW))).unwrap();
    assert_eq!(config.repositories[0].allowed_users, Some(expected));
    apply_change(&mut config, add(users(&["monalisa"])), Some(&agent(NEW))).unwrap();
    assert_eq!(
        config.repositories[0].allowed_users,
        Some(vec!["monalisa".to_owned()])
    );
    assert_eq!(config.repositories.len(), 1, "one repository, re-added");
}

#[test]
fn allow_replaces_or_clears_only_the_allowlist() {
    let mut config = config(json!([fixed("o/r", OLD), thread("o/t")]));
    config.repositories[0].enabled = false;
    let allow = |name: &str, allowed_users| WatcherChange::Allow {
        repository: repo(name),
        allowed_users,
    };
    apply_change(&mut config, allow("O/R", users(&["MonaLisa"])), None).unwrap();
    let narrowed = &config.repositories[0];
    assert_eq!(narrowed.allowed_users, Some(vec!["monalisa".to_owned()]));
    assert!(!narrowed.enabled, "an allowlist enables nothing");
    assert_eq!(narrowed.agent, Some(agent(OLD)), "nor retargets");
    apply_change(&mut config, allow("o/t", users(&["a"])), None).unwrap();
    assert!(
        config.repositories[1].allowed_users.is_some(),
        "any mode narrows"
    );
    apply_change(&mut config, allow("o/r", None), None).unwrap();
    assert_eq!(config.repositories[0].allowed_users, None);
    let missing = apply_change(&mut config, allow("o/missing", users(&["a"])), None);
    assert!(
        matches!(missing, Err(ArchonError::NotFound(_))),
        "{missing:?}"
    );
}

#[test]
fn configure_sets_only_what_it_names() {
    let mut config = config(json!([]));
    let change = WatcherChange::Configure {
        mention_regex: None,
        label_regex: Some("(?i)help".into()),
        poll_interval_secs: Some(300),
    };
    apply_change(&mut config, change, None).unwrap();
    assert_eq!(config.label_regex, "(?i)help");
    assert_eq!(config.poll_interval_secs, 300);
    assert_eq!(config.mention_regex, AETHER_MENTION_REGEX);
    let nothing = WatcherChange::Configure {
        mention_regex: None,
        label_regex: None,
        poll_interval_secs: None,
    };
    assert!(apply_change(&mut config, nothing, None).is_err());
}

#[test]
fn repointing_moves_only_fixed_repositories_and_is_idempotent() {
    let mut config = config(json!([fixed("o/a", OLD), fixed("o/b", NEW), thread("o/t")]));
    let before = config.repositories[2].clone();
    assert!(repoint(&mut config, &agent(NEW)));
    assert!(
        config.repositories[..2]
            .iter()
            .all(|r| r.agent == Some(agent(NEW)))
    );
    assert_eq!(
        config.repositories[2], before,
        "thread dispatch is untouched"
    );
    assert!(!repoint(&mut config, &agent(NEW)), "nothing left to move");
}

#[test]
fn the_view_names_each_target_and_joins_aethers_health() {
    let status = WatcherStatus {
        revision: 7,
        config: config(json!([fixed("o/a", NEW), fixed("o/b", OLD), thread("o/t")])),
        repositories: vec![RepositoryHealth {
            repository: "o/a".into(),
            last_poll_at: None,
            last_error: Some("rate limited".into()),
            dispatched: 3,
            ignored: 1,
        }],
    };
    let view = view(&status, Some(&agent(NEW)));
    assert_eq!(view.revision, 7);
    let targets: Vec<&WatchTarget> = view.repositories.iter().map(|r| &r.target).collect();
    assert_eq!(
        targets,
        [
            &WatchTarget::Orchestrator,
            &WatchTarget::Stale {
                agent: Some(agent(OLD))
            },
            &WatchTarget::Unmanaged {
                mode: "thread".into()
            },
        ]
    );
    assert_eq!(view.stale().count(), 1);
    assert_eq!(view.repositories[0].dispatched, 3);
    assert_eq!(view.repositories[1].dispatched, 0, "never polled yet");
    assert_eq!(view.repositories[0].allowed_users, None);
}

#[test]
fn the_view_shows_each_repositorys_allowlist() {
    let mut narrowed = fixed("o/a", NEW);
    narrowed["allowed_users"] = json!(["monalisa", "hubot"]);
    let status = WatcherStatus {
        revision: 2,
        config: config(json!([narrowed, fixed("o/b", NEW)])),
        repositories: Vec::new(),
    };
    let view = view(&status, Some(&agent(NEW)));
    assert_eq!(
        view.repositories[0].allowed_users,
        Some(vec!["monalisa".to_owned(), "hubot".to_owned()])
    );
    assert_eq!(view.repositories[1].allowed_users, None);
}

fn operators(logins: &[&str]) -> WatcherChange {
    WatcherChange::Operators {
        operator_users: Some(OperatorUsers::parse(logins).unwrap()),
    }
}

#[test]
fn operators_sets_or_clears_the_list_and_touches_nothing_else() {
    let mut config = config(json!([fixed("o/r", OLD), thread("o/t")]));
    let before = config.clone();
    apply_change(&mut config, operators(&["Octocat", "monaLisa"]), None).unwrap();
    let both = vec!["octocat".to_owned(), "monalisa".to_owned()];
    assert_eq!(config.operator_users, Some(both));
    assert_eq!(config.repositories, before.repositories);
    assert!(!config.enabled, "operator logins enable nothing");
    assert!(newly_delivering(&before, &config).is_empty());
    apply_change(&mut config, operators(&["monalisa"]), None).unwrap();
    assert_eq!(config.operator_users, Some(vec!["monalisa".to_owned()]));
    let clear = WatcherChange::Operators {
        operator_users: None,
    };
    apply_change(&mut config, clear, None).unwrap();
    assert_eq!(config, before, "cleared, the config is as it was");
}

#[test]
fn no_other_change_touches_the_operator_logins() {
    let mut config = config(json!([fixed("o/r", OLD), fixed("o/gone", OLD)]));
    let list = Some(vec!["octocat".to_owned(), "monalisa".to_owned()]);
    config.operator_users.clone_from(&list);
    let named = |name: &str| Some(repo(name));
    let changes = [
        WatcherChange::Add {
            repository: repo("o/new"),
            allowed_users: users(&["octocat"]),
        },
        WatcherChange::Allow {
            repository: repo("o/r"),
            allowed_users: users(&["someone"]),
        },
        WatcherChange::Allow {
            repository: repo("o/r"),
            allowed_users: None,
        },
        WatcherChange::Disable {
            repository: named("o/r"),
        },
        WatcherChange::Enable {
            repository: named("o/r"),
        },
        WatcherChange::Disable { repository: None },
        WatcherChange::Enable { repository: None },
        WatcherChange::Configure {
            mention_regex: Some("(?i)@bot".into()),
            label_regex: None,
            poll_interval_secs: Some(300),
        },
        WatcherChange::Remove {
            repository: repo("o/gone"),
        },
    ];
    for change in changes {
        apply_change(&mut config, change.clone(), Some(&agent(NEW))).unwrap();
        assert_eq!(config.operator_users, list, "{change:?}");
    }
    assert!(repoint(&mut config, &agent(OLD)));
    assert_eq!(config.operator_users, list, "repointing keeps it too");
    assert_eq!(config.repositories.len(), 2);
}

#[test]
fn the_view_names_the_operator_logins_each_allowlist_leaves_out() {
    let mut narrowed = fixed("o/narrow", NEW);
    narrowed["allowed_users"] = json!(["monalisa", "someone"]);
    let mut all = fixed("o/all", NEW);
    all["allowed_users"] = json!(["octocat", "monalisa"]);
    let mut status = WatcherStatus {
        revision: 3,
        config: config(json!([narrowed, all, fixed("o/open", NEW)])),
        repositories: Vec::new(),
    };
    let unset = view(&status, Some(&agent(NEW)));
    assert_eq!(unset.operator_users, None);
    assert!(
        unset
            .repositories
            .iter()
            .all(|r| r.excluded_operators.is_empty()),
        "no operator login, none left out"
    );
    status.config.operator_users = Some(vec!["octocat".into(), "monalisa".into()]);
    let set = view(&status, Some(&agent(NEW)));
    assert_eq!(set.operator_users, status.config.operator_users);
    assert_eq!(set.repositories[0].excluded_operators, ["octocat"]);
    assert!(set.repositories[1].excluded_operators.is_empty());
    assert!(
        set.repositories[2].excluded_operators.is_empty(),
        "no allowlist leaves nobody out"
    );
}

#[test]
fn a_write_must_come_back_with_the_operator_logins_it_sent() {
    let mut stored = config(json!([]));
    let sent = vec!["octocat".to_owned(), "monalisa".to_owned()];
    assert!(check_operators_stored(None, &stored).is_ok());
    let dropped = check_operators_stored(Some(&sent), &stored).unwrap_err();
    assert!(matches!(dropped, ArchonError::Protocol(_)), "{dropped}");
    stored.operator_users = Some(vec!["MonaLisa".into(), "octocat".into()]);
    assert!(
        check_operators_stored(Some(&sent), &stored).is_ok(),
        "the same logins in Aether's own order"
    );
    let appeared = check_operators_stored(None, &stored).unwrap_err();
    assert!(appeared.to_string().contains("MonaLisa"), "{appeared}");
    stored.operator_users = Some(vec!["octocat".into(), "monalisa".into(), "eve".into()]);
    assert!(
        check_operators_stored(Some(&sent), &stored).is_err(),
        "a wider list is not what was sent"
    );
}

#[test]
fn only_routes_that_start_delivering_need_a_prepared_target() {
    let mut before = config(json!([fixed("o/a", OLD), fixed("o/b", OLD), thread("o/t")]));
    before.enabled = true;
    let none = |after: &WatcherConfig| newly_delivering(&before, after).is_empty();
    assert!(none(&before), "no edit, nothing new");
    let mut after = before.clone();
    after.poll_interval_secs = 300;
    after.repositories.remove(1);
    after.repositories[0].enabled = false;
    after.enabled = true;
    assert!(
        none(&after),
        "configure, remove and disable deliver nothing new"
    );
    let mut off = before.clone();
    off.enabled = false;
    assert!(none(&off));
    assert_eq!(
        newly_delivering(&off, &before),
        [agent(OLD)].into(),
        "switching the watcher on delivers again"
    );
    let mut repointed = before.clone();
    repoint(&mut repointed, &agent(NEW));
    assert_eq!(newly_delivering(&before, &repointed), [agent(NEW)].into());
    repointed.enabled = false;
    assert!(none(&repointed), "a switched-off watcher delivers nothing");
    let mut renamed = before.clone();
    renamed.repositories[0].repository = "O/A".into();
    assert!(none(&renamed), "names compare as Aether stores them");
}

#[test]
fn every_enabled_fixed_route_of_a_running_watcher_delivers() {
    let routes = json!([
        fixed("o/a", OLD),
        fixed("o/b", NEW),
        fixed("o/c", NEW),
        thread("o/t")
    ]);
    let mut config = config(routes);
    assert!(
        delivering(&config).is_empty(),
        "a switched-off watcher delivers nothing"
    );
    config.enabled = true;
    assert_eq!(delivering(&config), [agent(OLD), agent(NEW)].into());
    assert!(
        newly_delivering(&config, &config).is_empty(),
        "all of them, not only the new ones"
    );
    config.repositories[0].enabled = false;
    assert_eq!(delivering(&config), [agent(NEW)].into());
    config.repositories.retain(|r| !r.is_fixed());
    assert!(
        delivering(&config).is_empty(),
        "thread dispatch has no fixed target"
    );
}
