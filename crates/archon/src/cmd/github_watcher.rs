//! `archon github-watcher [status | add | allow | operators | remove | enable |
//! disable | configure]`: Archon's watcher, aimed at the current orchestrator.

use std::process::ExitCode;

use archon_core::github::{
    AllowedUsers, GithubRepo, GithubWatcherView, OperatorUsers, WatcherChange,
};
use archon_core::protocol::{Reply, Request};

use super::{emit, to_json};
use crate::cli::GithubWatcherCommand;
use crate::connect::{connect_or_start, connect_running, unexpected};
use crate::format;

pub async fn run(command: Option<GithubWatcherCommand>, json: bool) -> anyhow::Result<ExitCode> {
    let change = change(command)?;
    let warning = change.as_ref().and_then(operators_warning);
    let (request, client) = match change {
        None => (Request::GithubWatcher, connect_running().await?),
        Some(change) => (
            Request::ConfigureGithubWatcher { change },
            connect_or_start().await?,
        ),
    };
    let view: Box<GithubWatcherView> = match client.request(request).await? {
        Reply::GithubWatcher(view) => view,
        other => return Err(unexpected(&other, "github_watcher")),
    };
    emit(json, || to_json(&view), &format::github_watcher(&view))?;
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
    Ok(ExitCode::SUCCESS)
}

/// What the operator is told once after `change` set operator logins.
fn operators_warning(change: &WatcherChange) -> Option<String> {
    let WatcherChange::Operators {
        operator_users: Some(users),
    } = change
    else {
        return None;
    };
    let logins = users.as_slice();
    let (have, they) = match logins.len() {
        1 => ("has", "it"),
        _ => ("have", "they"),
    };
    Some(format!(
        "Warning: {} now {have} your authority over Archon wherever {they} may dispatch. \
         Whoever controls a listed GitHub account, or a token that posts as it, can direct \
         Archon as you can. Clear the list with `archon github-watcher operators --clear` the \
         moment an account is in doubt.",
        logins.join(", "),
    ))
}

/// The edit a subcommand asks for; `None` for `status`, which edits nothing.
fn change(command: Option<GithubWatcherCommand>) -> anyhow::Result<Option<WatcherChange>> {
    let optional = |name: Option<String>| name.as_deref().map(repository).transpose();
    Ok(Some(match command {
        None | Some(GithubWatcherCommand::Status) => return Ok(None),
        Some(GithubWatcherCommand::Add {
            repository: name,
            allow,
        }) => WatcherChange::Add {
            repository: repository(&name)?,
            // No `--allow` keeps the repository's list, as Aether's `add` does.
            allowed_users: (!allow.is_empty()).then(|| allowlist(&allow)).transpose()?,
        },
        Some(GithubWatcherCommand::Allow {
            repository: name,
            logins,
            clear,
        }) => WatcherChange::Allow {
            repository: repository(&name)?,
            allowed_users: (!clear).then(|| allowlist(&logins)).transpose()?,
        },
        Some(GithubWatcherCommand::Operators { logins, clear }) => WatcherChange::Operators {
            operator_users: (!clear).then(|| operators(&logins)).transpose()?,
        },
        Some(GithubWatcherCommand::Remove { repository: name }) => WatcherChange::Remove {
            repository: repository(&name)?,
        },
        Some(GithubWatcherCommand::Enable { repository }) => WatcherChange::Enable {
            repository: optional(repository)?,
        },
        Some(GithubWatcherCommand::Disable { repository }) => WatcherChange::Disable {
            repository: optional(repository)?,
        },
        Some(GithubWatcherCommand::Configure {
            mention_regex,
            label_regex,
            poll_interval,
        }) => WatcherChange::Configure {
            mention_regex,
            label_regex,
            poll_interval_secs: poll_interval,
        },
    }))
}

fn repository(name: &str) -> anyhow::Result<GithubRepo> {
    GithubRepo::parse(name).map_err(anyhow::Error::from)
}

/// Checked here with Aether's rules, so a bad login never reaches archond.
fn allowlist(logins: &[String]) -> anyhow::Result<AllowedUsers> {
    AllowedUsers::parse(logins).map_err(anyhow::Error::from)
}

/// The allowlist's rules, checked here for the same reason.
fn operators(logins: &[String]) -> anyhow::Result<OperatorUsers> {
    OperatorUsers::parse(logins).map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;
    use crate::cli::{Cli, Command};

    fn parsed(args: &[&str]) -> anyhow::Result<Option<WatcherChange>> {
        let cli = Cli::try_parse_from([&["archon", "github-watcher"], args].concat())?;
        let Some(Command::GithubWatcher { command }) = cli.command else {
            panic!("{args:?} is not a github-watcher command");
        };
        change(command)
    }

    #[test]
    fn subcommands_become_watcher_edits_and_status_edits_nothing() {
        assert_eq!(parsed(&[]).unwrap(), None);
        assert_eq!(parsed(&["status"]).unwrap(), None);
        let Some(WatcherChange::Add {
            repository,
            allowed_users: None,
        }) = parsed(&["add", "Example-Org/Example-Repo"]).unwrap()
        else {
            panic!("add is an Add that keeps the allowlist");
        };
        assert_eq!(repository.as_str(), "example-org/example-repo");
        assert_eq!(parsed(&["list"]).unwrap(), None, "list is status");
        assert_eq!(
            parsed(&["disable"]).unwrap(),
            Some(WatcherChange::Disable { repository: None })
        );
        assert_eq!(
            parsed(&["configure", "--poll-interval", "300"]).unwrap(),
            Some(WatcherChange::Configure {
                mention_regex: None,
                label_regex: None,
                poll_interval_secs: Some(300),
            })
        );
    }

    fn allowlist_of(change: Option<WatcherChange>) -> Option<Vec<String>> {
        match change {
            Some(WatcherChange::Add { allowed_users, .. })
            | Some(WatcherChange::Allow { allowed_users, .. }) => allowed_users.map(Vec::from),
            other => panic!("{other:?} carries no allowlist"),
        }
    }

    #[test]
    fn allow_is_repeatable_or_comma_separated_and_lowercased() {
        let team = Some(vec!["monalisa".to_owned(), "hubot".to_owned()]);
        let repeated = ["add", "Example-Org/example-repo", "--allow", "monaLisa"];
        let repeated = [&repeated[..], &["--allow", "Hubot"]].concat();
        assert_eq!(allowlist_of(parsed(&repeated).unwrap()), team);
        let commas = ["add", "o/r", "--allow", "monaLisa,Hubot,MONALISA"];
        assert_eq!(allowlist_of(parsed(&commas).unwrap()), team);
        let replace = ["allow", "o/r", "monaLisa", "Hubot"];
        assert_eq!(allowlist_of(parsed(&replace).unwrap()), team);
        let mixed = ["allow", "o/r", "monaLisa,Hubot"];
        assert_eq!(allowlist_of(parsed(&mixed).unwrap()), team);
        let clear = parsed(&["allow", "O/R", "--clear"]).unwrap();
        let Some(WatcherChange::Allow { repository, .. }) = &clear else {
            panic!("allow is an Allow");
        };
        assert_eq!(repository.as_str(), "o/r");
        assert_eq!(allowlist_of(clear), None, "--clear removes the list");
    }

    #[test]
    fn bad_logins_and_ambiguous_allows_are_refused_here() {
        let bad = parsed(&["add", "o/r", "--allow", "ok,bad_login"]).unwrap_err();
        assert!(
            bad.to_string()
                .contains("invalid GitHub login \"bad_login\""),
            "{bad}"
        );
        let empty = parsed(&["add", "o/r", "--allow", ""]).unwrap_err();
        assert!(
            empty.to_string().contains("invalid GitHub login"),
            "{empty}"
        );
        let hyphen = parsed(&["allow", "o/r", "trailing-"]).unwrap_err();
        assert!(hyphen.to_string().contains("hyphen"), "{hyphen}");
        let over: Vec<String> = (0..=100).map(|n| format!("u{n}")).collect();
        let over = [vec!["allow".to_owned(), "o/r".to_owned()], over].concat();
        let over: Vec<&str> = over.iter().map(String::as_str).collect();
        let crowd = parsed(&over).unwrap_err();
        assert!(crowd.to_string().contains("at most 100"), "{crowd}");
        assert!(parsed(&["allow", "o/r"]).is_err(), "logins or --clear");
        assert!(
            parsed(&["allow", "o/r", "a", "--clear"]).is_err(),
            "not both"
        );
        assert!(parsed(&["allow", "not-a-repo", "a"]).is_err());
    }

    fn operators_of(args: &[&str]) -> Option<Vec<String>> {
        match parsed(args).unwrap() {
            Some(WatcherChange::Operators { operator_users }) => operator_users.map(Vec::from),
            other => panic!("{other:?} is not an operators edit"),
        }
    }

    #[test]
    fn operators_takes_a_list_or_clear_and_never_both() {
        let both = Some(vec!["octocat".to_owned(), "monalisa".to_owned()]);
        assert_eq!(operators_of(&["operators", "Octocat", "monaLisa"]), both);
        assert_eq!(operators_of(&["operators", "octocat,monaLisa"]), both);
        let repeats = ["operators", "octocat", "OCTOCAT,monalisa"];
        assert_eq!(operators_of(&repeats), both);
        assert_eq!(operators_of(&["operators", "--clear"]), None);
        assert!(parsed(&["operators"]).is_err(), "logins or --clear");
        assert!(
            parsed(&["operators", "octocat", "--clear"]).is_err(),
            "not both"
        );
        assert!(
            parsed(&["operators", "--clear", "octocat"]).is_err(),
            "in either order"
        );
    }

    #[test]
    fn bad_operator_logins_are_refused_here() {
        let bad = parsed(&["operators", "octocat", "bad_login"]).unwrap_err();
        assert!(
            bad.to_string()
                .contains("invalid GitHub login \"bad_login\""),
            "{bad}"
        );
        let bot = parsed(&["operators", "dependabot[bot]"]).unwrap_err();
        assert!(bot.to_string().contains("invalid GitHub login"), "{bot}");
        let empty = parsed(&["operators", ""]).unwrap_err();
        assert!(
            empty.to_string().contains("invalid GitHub login"),
            "{empty}"
        );
        let over: Vec<String> = (0..=100).map(|n| format!("u{n}")).collect();
        let over = [vec!["operators".to_owned()], over].concat();
        let over: Vec<&str> = over.iter().map(String::as_str).collect();
        let crowd = parsed(&over).unwrap_err();
        assert!(
            crowd
                .to_string()
                .contains("the operator list names 101 distinct logins; at most 100"),
            "{crowd}"
        );
    }

    #[test]
    fn only_setting_operator_logins_warns_and_names_them() {
        let warning = |args: &[&str]| parsed(args).unwrap().as_ref().and_then(operators_warning);
        let one = warning(&["operators", "Octocat"]).expect("setting warns");
        assert!(
            one.starts_with("Warning: octocat now has your authority over Archon"),
            "{one}"
        );
        assert!(one.contains("operators --clear"), "{one}");
        assert_eq!(one.lines().count(), 1, "one plain warning");
        let two = warning(&["operators", "octocat", "monalisa"]).expect("setting warns");
        assert!(two.contains("octocat, monalisa now have your authority"));
        for quiet in [
            &["operators", "--clear"][..],
            &["allow", "o/r", "octocat"],
            &["add", "o/r"],
            &["status"],
        ] {
            assert_eq!(warning(quiet), None, "{quiet:?}");
        }
    }

    #[test]
    fn bad_repositories_and_empty_configures_are_refused_here() {
        let url = parsed(&["add", "https://github.com/o/r"]).unwrap_err();
        assert!(url.to_string().contains("owner/name"), "{url}");
        assert!(parsed(&["configure"]).is_err(), "configure needs a setting");
        assert!(parsed(&["remove"]).is_err(), "remove needs a repository");
    }
}
