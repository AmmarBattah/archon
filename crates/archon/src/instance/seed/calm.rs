//! The instance's calm view definition; existing operator edits always win.

use std::io::{Read as _, Write as _};

mod definition;
use std::path::Path;

use anyhow::Context as _;

pub const FILE: &str = "modes/calm.toml";
pub const DEFINITION: &str = include_str!("calm.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Missing,
    Builtin,
    Edited,
    NotRegular,
}

impl State {
    pub fn description(self) -> &'static str {
        match self {
            Self::Missing => "missing; run `archon setup`",
            Self::Builtin => "installed",
            Self::Edited => "operator's own definition; unchanged",
            Self::NotRegular => {
                "not a regular mode file or modes directory; symlinks and other entries are left untouched"
            }
        }
    }
}

pub fn inspect(config_dir: &Path) -> anyhow::Result<State> {
    for path in [config_dir.join("modes"), config_dir.join(FILE)] {
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(State::Missing);
            }
            Err(error) => {
                return Err(error).with_context(|| format!("checking {}", path.display()));
            }
        };
        let valid = if path == config_dir.join("modes") {
            meta.is_dir()
        } else {
            meta.is_file()
        };
        if !valid {
            return Ok(State::NotRegular);
        }
    }
    let mut mode_count = 0;
    for entry in std::fs::read_dir(config_dir.join("modes")).context("reading mode catalog")? {
        let path = entry.context("reading mode catalog entry")?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            mode_count += 1;
            anyhow::ensure!(
                mode_count <= 128,
                "more than 128 mode files; Aether rejects the external catalog"
            );
        }
    }
    let path = config_dir.join(FILE);
    let mut text = String::new();
    std::fs::File::open(&path)
        .with_context(|| format!("opening {}", path.display()))?
        .take(65_537)
        .read_to_string(&mut text)
        .with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(
        text.len() <= 65_536,
        "{}: mode exceeds 64 KiB",
        path.display()
    );
    definition::validate(&text).with_context(|| format!("validating {}", path.display()))?;
    Ok(if text == DEFINITION {
        State::Builtin
    } else {
        State::Edited
    })
}

/// Publish a complete file without replacing any existing entry, even a broken link.
pub fn seed(config_dir: &Path) -> anyhow::Result<&'static str> {
    let state = inspect(config_dir)?;
    if state != State::Missing {
        return Ok(state.description());
    }
    let dir = config_dir.join("modes");
    super::super::create_private_dir(&dir)?;
    let path = config_dir.join(FILE);
    let mut staged = tempfile::NamedTempFile::new_in(&dir)
        .with_context(|| format!("staging {}", path.display()))?;
    staged
        .write_all(DEFINITION.as_bytes())
        .and_then(|()| staged.as_file().sync_all())
        .with_context(|| format!("writing {}", path.display()))?;
    match staged.persist_noclobber(&path) {
        Ok(_) => Ok("seeded"),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(inspect(config_dir)?.description())
        }
        Err(error) => Err(error.error).with_context(|| format!("installing {}", path.display())),
    }
}
