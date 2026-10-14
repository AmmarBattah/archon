use std::path::Path;

use super::*;
use crate::arms::tests::settings;
use crate::metrics::tests::result;

fn options(arms: Vec<Arm>, trials: u32) -> RunOptions {
    RunOptions {
        arms,
        trials,
        parallel: 2,
        max_usd: 10.0,
        max_trials: 100,
        timeout_factor: 1.0,
        work_dir: "/data/work".into(),
        results_dir: "/w/results".into(),
        stamp: "20260926T101500Z".into(),
        keep_workspaces: false,
    }
}

fn bundled() -> Vec<BenchTask> {
    BenchTask::load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks")).unwrap()
}

#[test]
fn trials_interleave_arms_within_each_round() {
    let specs = plan(2, &[Arm::Claude, Arm::Codex], 2);
    let order: Vec<(u32, usize, Arm)> = specs.iter().map(|s| (s.trial, s.task, s.arm)).collect();
    assert_eq!(
        order,
        [
            (1, 0, Arm::Claude),
            (1, 0, Arm::Codex),
            (1, 1, Arm::Claude),
            (1, 1, Arm::Codex),
            (2, 0, Arm::Claude),
            (2, 0, Arm::Codex),
            (2, 1, Arm::Claude),
            (2, 1, Arm::Codex),
        ]
    );
}

#[test]
fn max_trials_refuses_an_oversized_plan() {
    let mut opts = options(Arm::ALL.to_vec(), 3);
    let specs = plan(9, &opts.arms, opts.trials);
    opts.max_trials = 108;
    assert!(check_plan(&specs, &opts).is_ok());
    opts.max_trials = 107;
    let error = check_plan(&specs, &opts).unwrap_err().to_string();
    assert!(error.contains("108 trials"), "{error}");
    assert!(check_plan(&[], &opts).is_err());
}

#[test]
fn options_reject_nonsense() {
    assert!(options(vec![Arm::Claude], 1).validate().is_ok());
    assert!(options(Vec::new(), 1).validate().is_err());
    assert!(
        options(vec![Arm::Claude, Arm::Claude], 1)
            .validate()
            .is_err()
    );
    assert!(options(vec![Arm::Claude], 0).validate().is_err());
    let mut free = options(vec![Arm::Claude], 1);
    free.max_usd = 0.0;
    assert!(free.validate().is_err());
}

#[test]
fn budget_stops_new_trials_once_spent() {
    let mut budget = Budget::new(1.0);
    assert!(budget.allows_another());
    budget.record(&result(Some(0.6), None));
    budget.record(&result(None, None));
    assert!(budget.allows_another());
    budget.record(&result(None, Some(0.5)));
    assert!(!budget.allows_another());
    assert!((budget.spent_usd - 1.1).abs() < 1e-9);
    assert_eq!(budget.unpriced_trials, 1);
}

#[test]
fn trial_logs_nest_by_stamp_task_arm_and_trial_away_from_workspaces() {
    let opts = options(vec![Arm::Codex], 1);
    assert_eq!(
        opts.trial_logs_dir("t", Arm::Codex, 3),
        Path::new("/w/results/20260926T101500Z/trials/t/codex/3/logs")
    );
    assert_eq!(
        opts.run_work_dir(),
        Path::new("/data/work/20260926T101500Z")
    );
    assert_eq!(
        opts.run_results_dir(),
        Path::new("/w/results/20260926T101500Z")
    );
}

#[test]
fn dry_run_prints_every_arm_command_without_touching_disk() {
    let tasks = bundled();
    let opts = options(Arm::ALL.to_vec(), 2);
    let specs = plan(tasks.len(), &opts.arms, opts.trials);
    let versions = vec![(Arm::Claude, "2.1.280 (Claude Code)".to_owned())];
    let text = render_dry_run(&tasks, &specs, &settings(), &opts, &versions).unwrap();
    assert!(text.starts_with(&format!("plan: {} tasks x 4 arms x 2 trials", tasks.len())));
    assert!(text.contains("2.1.280 (Claude Code)"));
    assert!(text.contains("--dangerously-skip-permissions"));
    assert!(text.contains("exec --json"));
    assert!(text.contains("aether create_agent"));
    assert!(text.contains("daemon run"));
    assert!(text.contains("repo_add"));
    for task in &tasks {
        assert!(text.contains(&format!("== {} (", task.file.id)));
    }
    assert!(text.contains("/data/work/20260926T101500Z/<random>/fix-off-by-one"));
    assert!(!Path::new("/data/work").exists());
}

#[test]
fn the_default_work_dir_is_outside_the_checkout() {
    let dir = |xdg: Option<&str>, home: Option<&str>| {
        work_dir_from(xdg.map(Into::into), home.map(Into::into))
    };
    assert_eq!(
        dir(Some("/x/data"), Some("/users/u")).unwrap(),
        Path::new("/x/data/archon-bench/work")
    );
    assert_eq!(
        dir(Some("relative"), Some("/users/u")).unwrap(),
        Path::new("/users/u/.local/share/archon-bench/work")
    );
    assert!(dir(None, None).is_err());
    let tasks = Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks");
    let protected = crate::isolation::Protected::for_run(&tasks, &default_results_dir()).unwrap();
    let old_default = Path::new(env!("CARGO_MANIFEST_DIR")).join(".work");
    assert!(protected.check_work_dir(&old_default).is_err());
}
