//! The stage contract (ARCHITECTURE §8.6): how every stage agent must work
//! and report, passed as `config.instructions` so pipeline authors cannot
//! omit it. The stage prompt says *what* to do; this says *how to finish*.

use archon_core::catalog::{Gate, StageKind, WorkspaceMode};
use archon_core::run::StageOutcome;

use crate::spec::StageSpec;
use crate::team::TeamSlot;

/// Where a stage stands towards its pipeline's team stage.
#[derive(Debug, Clone, Copy)]
pub enum TeamPart<'a> {
    /// The pipeline has no team stage.
    Solo,
    /// Another stage of a team pipeline, in the run worktree on `branch`.
    RunTree { branch: &'a str },
    /// One role's slot of the team stage.
    Role(&'a TeamSlot),
}

/// The contract for one stage agent working in `workspace`.
pub fn instructions(
    stage: &StageSpec,
    mode: WorkspaceMode,
    workspace: &str,
    team: TeamPart<'_>,
) -> String {
    assert!(!workspace.is_empty(), "a stage always has a workspace");
    assert!(stage.fan_out >= 1, "validated specs have fan_out >= 1");
    assert_eq!(
        matches!(team, TeamPart::Role(_)),
        stage.kind == StageKind::Team,
        "a team slot comes with a team stage, and only with one"
    );
    let blocked = StageOutcome::BLOCKED_PREFIX;
    let mut out = format!(
        "# Archon stage contract\n\n\
         You are one stage (`{id}`, kind `{kind}`) of an Archon pipeline, running \
         unattended. Archon reads your result mechanically, so follow this contract \
         exactly. It overrides any conflicting habit or instruction.\n\n\
         ## Workspace\n\n\
         - Work only in `{workspace}`. {mode_rule}\n\
         - Do not ask the operator questions and do not wait for replies: nobody is \
         watching this agent. Make the conservative choice, and state every assumption \
         in your summary.\n\
         - Use `mcp__archon__message_orchestrator` to relay operator messages meant for \
         the orchestrator (`kind=relay`, quote their text exactly), escalate blockers \
         outside your stage (`kind=escalation`), or send low-priority `info`. It returns \
         a receipt immediately. Keep working on your stage afterwards: never wait for \
         a reply, end your turn, or complete the stage because you sent a message. \
         Your passive run-node parent cannot answer; do not use raw `send_message` \
         to reach the orchestrator. Other Archon mutation tools are reserved for it.\n\
         {fan_rule}\n\
         ## Finishing\n\n\
         - When done, call Aether's `complete_task` exactly once with a concise \
         `summary`: plain prose, what you did or found, how you verified it, and what \
         is left undone. Later stages and the orchestrator read only this.\n\
         - If you cannot proceed without a human (missing credentials or access, a \
         contradictory or ambiguous brief with no safe default, an irreversible action \
         the brief does not authorise), call `fail_task` with an error that starts \
         with `{blocked}` followed by exactly what the operator must provide or decide. \
         This escalates to the operator and is never retried automatically.\n\
         - For any other failure, call `fail_task` with a precise error: what failed, \
         what you tried, and the evidence. Never report a failure as completed.\n",
        id = stage.id,
        kind = kind_name(stage.kind),
        mode_rule = mode_rule(mode),
        fan_rule = match team {
            TeamPart::Role(slot) => team_rule(stage, slot),
            TeamPart::Solo | TeamPart::RunTree { .. } => fan_rule(stage),
        },
    );
    out.push_str(kind_rules(stage.kind));
    if let TeamPart::RunTree { branch } = team
        && stage.kind == StageKind::Work
    {
        out.push_str(&run_tree_rule(branch));
    }
    if needs_verdict(stage) {
        out.push_str(VERDICT_RULES);
    }
    out
}

/// Whether the stage's result must carry a verdict: always for review and
/// test stages, and for any gated stage.
pub fn needs_verdict(stage: &StageSpec) -> bool {
    matches!(stage.kind, StageKind::Review | StageKind::Test) || stage.gate != Gate::None
}

fn kind_name(kind: StageKind) -> &'static str {
    match kind {
        StageKind::Propose => "propose",
        StageKind::Work => "work",
        StageKind::Review => "review",
        StageKind::Test => "test",
        StageKind::Ship => "ship",
        StageKind::Report => "report",
        StageKind::Team => "team",
    }
}

fn mode_rule(mode: WorkspaceMode) -> &'static str {
    match mode {
        WorkspaceMode::Worktree => {
            "It is a git worktree dedicated to this pipeline run; earlier and later \
             stages of the same run work in it too. Do not touch other worktrees, other \
             branches, or the operator's own checkout."
        }
        WorkspaceMode::RepoReadonly => {
            "With a repository, it is a detached snapshot of the commit the repository has \
             checked out, made for this run and deleted when it ends; the operator's own \
             checkout is elsewhere, and its uncommitted changes are not in it. It is \
             READ-ONLY for you: do not create, edit or delete files, do not switch branches, \
             commit or stash, and do not run commands that write into it (formatters, \
             installers, in-tree builds). Put any scratch files in your own temporary \
             directory outside it."
        }
        WorkspaceMode::Scratch => {
            "It is a scratch directory for this item only; nothing in it is kept or \
             shipped."
        }
    }
}

fn fan_rule(stage: &StageSpec) -> String {
    if stage.fan_out <= 1 {
        return String::new();
    }
    format!(
        "- You are one of {} agents running this stage in parallel on the same \
         workspace. Work independently, do not coordinate with the others, and do not \
         modify tracked files.\n",
        stage.fan_out
    )
}

fn team_rule(stage: &StageSpec, slot: &TeamSlot) -> String {
    assert!(
        stage.roles.contains(&slot.role),
        "the slot's role is the stage's"
    );
    format!(
        "- You are role `{role}` of roles [{roles}] of this team stage. The other roles \
         run in parallel, each in its own worktree; never edit theirs. Stay within your \
         role.\n\
         - Commit your work on branch `{branch}`, checked out in your worktree. Archon \
         merges every role's branch onto the run branch once all roles are done; \
         anything you leave uncommitted is committed there for you, except nested git \
         repositories and changes to tracked files inside submodules, which stop the \
         merge. Add submodules only with `git submodule add`. Do not push, \
         do not switch branches, and finish any merge or rebase you start.\n",
        role = slot.role,
        roles = stage.roles.join(", "),
        branch = slot.branch,
    )
}

/// A work stage of a team pipeline: the roles see only what it committed.
fn run_tree_rule(branch: &str) -> String {
    assert!(!branch.is_empty(), "team runs have a run branch");
    format!(
        "- This pipeline has a team stage. Its roles fork their branches from the last \
         commit on the run branch `{branch}`, and Archon merges them back onto it. Commit \
         your changes on `{branch}` before you finish: the roles see only what is \
         committed there, and uncommitted changes to tracked files stop the team stage. \
         Add submodules only with `git submodule add`: a repository cloned or created \
         in place and committed stops the team stage too. Do not push, and do not \
         switch branches.\n"
    )
}

fn kind_rules(kind: StageKind) -> &'static str {
    match kind {
        StageKind::Propose => PROPOSE_RULES,
        StageKind::Work => WORK_RULES,
        StageKind::Review => REVIEW_RULES,
        StageKind::Test => TEST_RULES,
        StageKind::Ship => SHIP_RULES,
        StageKind::Report => REPORT_RULES,
        StageKind::Team => TEAM_RULES,
    }
}

const PROPOSE_RULES: &str = "\n## Propose stage\n\n\
- Propose only: read what you need, but do not edit, commit or push anything. \
Write nowhere in the workspace except `<workspace>/.archon/reports/proposal/`.\n\
- Write the proposal as `<workspace>/.archon/reports/proposal/index.html`, one \
self-contained page that loads nothing from the network.\n\
- `complete_task` must include `structured` data listing the page's absolute path: \
`{\"reports\": [\"/abs/…/index.html\"]}`. The operator may approve the proposal, \
so the `summary` must state the approach on its own.\n";

const WORK_RULES: &str = "\n## Work stage\n\n\
- Your summary is the hand-off to later stages and the orchestrator. Use the report \
format the stage prompt gives. If it gives none, list what changed and why, the files \
touched, the exact verification commands you ran with their results, and any known \
gaps.\n";

const TEAM_RULES: &str = "\n## Team stage\n\n\
- Your summary is the hand-off to later stages and the orchestrator. Use the report \
format the stage prompt gives. If it gives none, list what your role changed and why, \
the files touched, the commits you made, the verification commands you ran with their \
results, any assumption you made about a peer's work, and any known gaps.\n\
\n## Team coordination\n\n\
- Once every role has an agent, Archon sends you one roster message: each role's name, \
Aether agent id, worktree and branch. If a role is replaced, a roster with a higher \
version follows; it supersedes the earlier one.\n\
- Your peers are other agents, not the operator: message them with Aether's \
`send_message` and the agent ids from the roster. Agree interfaces and contracts with \
the roles that depend on them early, before you build on them. Report a breaking \
change to the roles it affects as soon as you make it. Ask a peer for help when you \
debug something in its scope.\n\
- Keep messages concise: what you need or changed, with exact names, paths and \
signatures.\n\
- Stay within your role's scope and your own worktree. Never edit, commit in or run \
git in a peer's worktree; ask its role instead.\n\
- Never block on a peer for long. If no answer comes, proceed on your best assumption \
and state it in your summary.\n\
- Commit your work on your own branch: peers see it only once Archon merges every \
role's branch.\n";

const REVIEW_RULES: &str = "\n## Review stage\n\n\
- Review only; do not edit, commit or push. Your findings go back to the implementer \
verbatim if the gate fails, so make each one actionable: where (path and line), what \
is wrong, why it matters, and what a fix looks like.\n";

const TEST_RULES: &str = "\n## Test stage\n\n\
- Verify by running things, not by reading alone. Do not edit tracked files to make a \
check pass; a failing check is a finding. Build outputs and caches are fine.\n\
- Quote the failing command and the relevant output lines in each finding's `detail`.\n";

const SHIP_RULES: &str = "\n## Ship stage\n\n\
- Deliver exactly as the stage prompt's ship mode says. Never force-push, never \
rewrite history that already left this machine, and never merge unless the prompt \
says auto-merge is enabled.\n\
- `complete_task` must include `structured` data naming what was delivered: \
`{\"pr_url\": \"https://…\"}` when a pull request exists, otherwise \
`{\"commit\": \"<full sha>\"}` for the commit on the run branch. Include both when \
both apply.\n";

const REPORT_RULES: &str = "\n## Report stage\n\n\
- Write each HTML report as `<workspace>/.archon/reports/<name>/index.html` with its \
assets (screenshots, CSS) in the same directory, using relative links.\n\
- `complete_task` must include `structured` data listing the absolute path of every \
report you want published: `{\"reports\": [\"/abs/…/index.html\"]}`. Use an empty \
list if you wrote none. Archon publishes exactly these files.\n";

const VERDICT_RULES: &str = "\n## Verdict\n\n\
- `complete_task` must include `structured` data of this shape (other keys may sit \
beside it):\n\n\
```json\n\
{\"verdict\": \"pass\" | \"fail\",\n \
\"findings\": [{\"severity\": \"critical|high|medium|low\", \"title\": \"…\", \
\"detail\": \"…\", \"path\": \"relative/path:line\"}]}\n\
```\n\n\
- The stage prompt defines which findings make the verdict `fail`; apply its rule \
exactly. A `fail` lists at least one finding. A `pass` may still list findings the \
prompt treats as advice.\n\
- End the `summary` with a final line that is exactly `VERDICT: PASS` or \
`VERDICT: FAIL`, matching the structured verdict. A missing or unreadable verdict \
counts as a fail.\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(kind: StageKind, gate: Gate, fan_out: u32) -> StageSpec {
        StageSpec {
            id: "s".into(),
            kind,
            prompt: "s.md".into(),
            fan_out,
            gate,
            timeout_minutes: 10,
            allow_delegation: false,
            nudge: None,
            on_fail: None,
            roles: Vec::new(),
            role_prompts: Default::default(),
        }
    }

    #[test]
    fn every_contract_names_the_workspace_and_blocked_prefix() {
        for kind in [
            StageKind::Propose,
            StageKind::Work,
            StageKind::Review,
            StageKind::Test,
            StageKind::Ship,
            StageKind::Report,
        ] {
            let text = instructions(
                &stage(kind, Gate::None, 1),
                WorkspaceMode::Worktree,
                "/w",
                TeamPart::Solo,
            );
            assert!(text.contains("Work only in `/w`"), "{kind:?}");
            assert!(text.contains("`BLOCKED:`"), "{kind:?}");
            assert!(text.contains("complete_task"), "{kind:?}");
            assert!(
                text.contains("mcp__archon__message_orchestrator"),
                "{kind:?}"
            );
            assert!(
                text.contains("Keep working on your stage afterwards"),
                "{kind:?}"
            );
            assert!(
                text.contains("never wait for a reply, end your turn"),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn review_and_test_contracts_demand_a_verdict() {
        for kind in [StageKind::Review, StageKind::Test] {
            let text = instructions(
                &stage(kind, Gate::AllPass, 1),
                WorkspaceMode::Worktree,
                "/w",
                TeamPart::Solo,
            );
            assert!(text.contains("VERDICT: PASS"), "{kind:?}");
            assert!(text.contains("\"verdict\""), "{kind:?}");
            // The contract fixes the format; the prompt owns the threshold.
            assert!(text.contains("stage prompt defines which findings"));
        }
        let work = instructions(
            &stage(StageKind::Work, Gate::None, 1),
            WorkspaceMode::Worktree,
            "/w",
            TeamPart::Solo,
        );
        assert!(!work.contains("VERDICT: PASS"));
    }

    #[test]
    fn report_contract_asks_for_verdict_only_when_gated() {
        let ungated = stage(StageKind::Report, Gate::None, 2);
        let gated = stage(StageKind::Report, Gate::Majority, 2);
        let text = instructions(&ungated, WorkspaceMode::Worktree, "/w", TeamPart::Solo);
        assert!(text.contains("\"reports\""));
        assert!(text.contains("one of 2 agents"));
        assert!(!text.contains("VERDICT: PASS"));
        assert!(
            instructions(&gated, WorkspaceMode::Worktree, "/w", TeamPart::Solo)
                .contains("VERDICT: PASS")
        );
    }

    #[test]
    fn ship_contract_names_structured_delivery_keys() {
        let text = instructions(
            &stage(StageKind::Ship, Gate::None, 1),
            WorkspaceMode::Worktree,
            "/w",
            TeamPart::Solo,
        );
        assert!(text.contains("\"pr_url\""));
        assert!(text.contains("\"commit\""));
    }

    #[test]
    fn propose_contract_forbids_edits_and_names_the_page() {
        let text = instructions(
            &stage(StageKind::Propose, Gate::None, 1),
            WorkspaceMode::Worktree,
            "/w",
            TeamPart::Solo,
        );
        assert!(text.contains("kind `propose`"));
        assert!(text.contains(".archon/reports/proposal/index.html"));
        assert!(!text.contains("VERDICT: PASS"));
    }

    #[test]
    fn team_contract_names_the_role_roster_and_branch() {
        let mut team = stage(StageKind::Team, Gate::None, 2);
        team.roles = vec!["api".into(), "ui".into()];
        let slot = TeamSlot {
            role: "ui".into(),
            branch: "archon/x--ui-q69g5fav-a1".into(),
            workspace: "/t/s-ui".into(),
        };
        let part = TeamPart::Role(&slot);
        let text = instructions(&team, WorkspaceMode::Worktree, "/t/s-ui", part);
        assert!(text.contains("kind `team`"));
        assert!(text.contains("role `ui` of roles [api, ui]"), "{text}");
        assert!(text.contains("Commit your work on branch `archon/x--ui-q69g5fav-a1`"));
        assert!(text.contains("Stay within your role"));
        assert!(!text.contains("do not modify tracked files"));
    }

    #[test]
    fn team_contract_explains_the_roster_and_how_to_coordinate() {
        let mut team = stage(StageKind::Team, Gate::None, 2);
        team.roles = vec!["api".into(), "ui".into()];
        let slot = TeamSlot {
            role: "api".into(),
            branch: "archon/x--api-q69g5fav-a1".into(),
            workspace: "/t/s-api".into(),
        };
        let text = instructions(
            &team,
            WorkspaceMode::Worktree,
            "/t/s-api",
            TeamPart::Role(&slot),
        );
        for rule in [
            "Archon sends you one roster message",
            "a roster with a higher version follows; it supersedes",
            "`send_message`",
            "Agree interfaces and contracts",
            "Report a breaking change",
            "Ask a peer for help when you debug",
            "Keep messages concise",
            "Never edit, commit in or run git in a peer's worktree",
            "proceed on your best assumption and state it in your summary",
            "Commit your work on your own branch",
        ] {
            assert!(text.contains(rule), "missing {rule:?} in {text}");
        }
        let solo = instructions(
            &stage(StageKind::Work, Gate::None, 1),
            WorkspaceMode::Worktree,
            "/w",
            TeamPart::Solo,
        );
        assert!(!solo.contains("roster"), "only team roles get a roster");
    }

    #[test]
    fn work_stages_of_a_team_pipeline_must_commit() {
        let part = TeamPart::RunTree { branch: "archon/x" };
        for kind in [StageKind::Work, StageKind::Review, StageKind::Ship] {
            let gate = match kind {
                StageKind::Review => Gate::AllPass,
                _ => Gate::None,
            };
            let text = instructions(
                &stage(kind, gate, 1),
                WorkspaceMode::Worktree,
                "/t/main",
                part,
            );
            let rule = text.contains("Commit your changes on `archon/x` before you finish");
            assert_eq!(rule, kind == StageKind::Work, "{kind:?}");
            let submodules = text.contains("Add submodules only with `git submodule add`");
            assert_eq!(submodules, kind == StageKind::Work, "{kind:?}");
        }
        let solo = instructions(
            &stage(StageKind::Work, Gate::None, 1),
            WorkspaceMode::Worktree,
            "/w",
            TeamPart::Solo,
        );
        assert!(!solo.contains("team stage"));
    }

    #[test]
    fn readonly_mode_forbids_edits() {
        let text = instructions(
            &stage(StageKind::Work, Gate::None, 1),
            WorkspaceMode::RepoReadonly,
            "/r",
            TeamPart::Solo,
        );
        assert!(text.contains("READ-ONLY"));
        assert!(
            text.contains("snapshot"),
            "it is not the operator's checkout"
        );
    }
}
