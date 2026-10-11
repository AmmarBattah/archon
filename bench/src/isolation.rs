//! What agents must not reach: the Archon checkout, the task suite (hidden
//! tests, reference solutions, rubrics) and earlier results. Trial
//! workspaces must live outside all of them, and agent processes must not
//! inherit environment variables that point into them (`cargo run` alone
//! exports `CARGO_MANIFEST_DIR=<checkout>/bench`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

use crate::process::CommandSpec;

/// Variables removed from every agent process whatever their value.
const ALWAYS_REMOVED: &[&str] = &["OLDPWD", "PWD"];
/// Cargo exports these to programs it runs. `CARGO_HOME` (the registry
/// cache) and `CARGO_BUILD_JOBS` (the host's CPU limit) hold no checkout path.
const CARGO_PREFIX: &str = "CARGO_";
const CARGO_KEPT: &[&str] = &["CARGO_HOME", "CARGO_BUILD_JOBS"];

/// Environment changes applied to every agent-side process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvScrub {
    pub remove: Vec<String>,
    pub set: Vec<(String, String)>,
}

impl EnvScrub {
    pub fn apply(&self, spec: &mut CommandSpec) {
        spec.env_remove.extend(self.remove.iter().cloned());
        spec.env.extend(self.set.iter().cloned());
    }
}

/// Directories agents must not be able to reach by walking up from their
/// workspace or by reading their environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protected {
    roots: Vec<PathBuf>,
}

impl Protected {
    pub fn new(roots: &[&Path]) -> anyhow::Result<Self> {
        ensure!(!roots.is_empty(), "nothing to protect");
        let roots = roots
            .iter()
            .map(|root| resolve(root))
            .collect::<anyhow::Result<Vec<_>>>()?;
        assert!(roots.iter().all(|r| r.is_absolute()), "roots are resolved");
        Ok(Self { roots })
    }

    /// The checkout this binary was built from, the task suite and the
    /// results directory.
    pub fn for_run(tasks: &Path, results: &Path) -> anyhow::Result<Self> {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let checkout = manifest.parent().unwrap_or(manifest);
        Self::new(&[checkout, tasks, results])
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Trial workspaces must sit outside every protected root, and outside
    /// the system temp directory (a small shared tmpfs on the bench host).
    pub fn check_work_dir(&self, work_dir: &Path) -> anyhow::Result<()> {
        let dir = resolve(work_dir)?;
        for root in &self.roots {
            ensure!(
                !dir.starts_with(root),
                "--work-dir {} is inside {}: agents could walk up to the hidden tests and \
                 references; choose a directory outside it",
                dir.display(),
                root.display()
            );
        }
        let temp = resolve(&std::env::temp_dir())?;
        ensure!(
            !dir.starts_with(&temp),
            "--work-dir {} is under {}, a small shared tmpfs; choose a directory elsewhere",
            dir.display(),
            temp.display()
        );
        Ok(())
    }

    /// What to remove or rewrite in `vars` (the harness's own environment)
    /// before an agent process inherits it.
    pub fn scrub_env(&self, vars: impl IntoIterator<Item = (OsString, OsString)>) -> EnvScrub {
        let mut scrub = EnvScrub::default();
        for (name, value) in vars {
            let (Some(name), value) = (name.to_str(), value.to_string_lossy()) else {
                continue;
            };
            if name == "PATH" {
                if let Some(kept) = self.filtered_path(&value) {
                    scrub.set.push((name.to_owned(), kept));
                }
                continue;
            }
            let cargo = name.starts_with(CARGO_PREFIX) && !CARGO_KEPT.contains(&name);
            let points_inside = self
                .roots
                .iter()
                .any(|root| value.contains(root.to_string_lossy().as_ref()));
            if ALWAYS_REMOVED.contains(&name) || cargo || points_inside {
                scrub.remove.push(name.to_owned());
            }
        }
        scrub.remove.sort();
        scrub
    }

    /// `PATH` without entries inside a protected root; `None` if unchanged.
    fn filtered_path(&self, path: &str) -> Option<String> {
        let entries: Vec<&str> = path.split(':').collect();
        let kept: Vec<&str> = entries
            .iter()
            .copied()
            .filter(|entry| !self.roots.iter().any(|r| Path::new(entry).starts_with(r)))
            .collect();
        (kept.len() != entries.len()).then(|| kept.join(":"))
    }
}

/// Absolute and symlink-free, even when the tail does not exist yet.
fn resolve(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute =
        std::path::absolute(path).with_context(|| format!("making {} absolute", path.display()))?;
    let existing = absolute
        .ancestors()
        .find(|p| p.exists())
        .context("no ancestor exists, not even /")?;
    let rest = absolute
        .strip_prefix(existing)
        .expect("an ancestor is a prefix");
    let canonical = existing
        .canonicalize()
        .with_context(|| format!("resolving {}", existing.display()))?;
    Ok(canonical.join(rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::scratch;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn work_dirs_inside_protected_roots_or_temp_are_refused() {
        let tmp = scratch();
        let checkout = tmp.path().join("archon");
        std::fs::create_dir_all(checkout.join("bench/tasks")).unwrap();
        let protected = Protected::new(&[&checkout]).unwrap();
        let inside = checkout.join("bench/.work");
        let error = protected.check_work_dir(&inside).unwrap_err().to_string();
        assert!(error.contains("walk up"), "{error}");
        assert!(
            protected
                .check_work_dir(&checkout.join("../elsewhere/work"))
                .is_ok()
        );
        let temp = std::env::temp_dir().join("archon-bench-work");
        assert!(protected.check_work_dir(&temp).is_err());
    }

    #[test]
    fn scrub_removes_everything_pointing_at_the_checkout() {
        // Relative to the working directory, which need not be the checkout
        // this binary was built from when builds share a target directory.
        let cwd = std::env::current_dir().unwrap();
        let relative = Protected::new(&[Path::new("src/../src")]).unwrap();
        assert_eq!(relative.roots(), [cwd.join("src").canonicalize().unwrap()]);
        let protected = Protected {
            roots: vec!["/home/u/archon".into(), "/data/results".into()],
        };
        let scrub = protected.scrub_env(vars(&[
            ("CARGO_MANIFEST_DIR", "/home/u/archon/bench"),
            ("CARGO_PKG_NAME", "archon-bench"),
            ("CARGO_HOME", "/home/u/.cargo"),
            ("CARGO_BUILD_JOBS", "4"),
            ("CARGO_TARGET_DIR", "/shared/target"),
            ("OLDPWD", "/somewhere"),
            ("NOTES", "see /data/results/20260926"),
            ("HOME", "/home/u"),
            (
                "PATH",
                "/home/u/archon/target/debug:/usr/bin:/home/u/.cargo/bin",
            ),
            ("ANTHROPIC_API_KEY", "sk-test"),
        ]));
        assert_eq!(
            scrub.remove,
            [
                "CARGO_MANIFEST_DIR",
                "CARGO_PKG_NAME",
                "CARGO_TARGET_DIR",
                "NOTES",
                "OLDPWD"
            ]
        );
        assert_eq!(
            scrub.set,
            [("PATH".to_owned(), "/usr/bin:/home/u/.cargo/bin".to_owned())]
        );
        let untouched = protected.scrub_env(vars(&[("PATH", "/usr/bin")]));
        assert!(untouched.set.is_empty() && untouched.remove.is_empty());
    }

    #[test]
    fn scrub_is_applied_to_a_command() {
        let scrub = EnvScrub {
            remove: vec!["CARGO_MANIFEST_DIR".into()],
            set: vec![("PATH".into(), "/usr/bin".into())],
        };
        let mut spec = CommandSpec::new("claude", "/w");
        scrub.apply(&mut spec);
        assert_eq!(spec.env_remove, ["CARGO_MANIFEST_DIR"]);
        assert_eq!(spec.env, [("PATH".to_owned(), "/usr/bin".to_owned())]);
    }
}
