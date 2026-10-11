//! Whether the installed `aether` serves a protocol an archon speaks: the
//! ranges of `archon update --requirements` and of `aether schema`.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use archon_aether::wire::{MIN_PROTOCOL_VERSION, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

use super::super::doctor::find_program;
use super::tools;
use crate::instance::AETHER_PROGRAM;

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const MIN_KEY: &str = "aether_protocol_min";
const MAX_KEY: &str = "aether_protocol_max";

/// The Aether protocols an archon speaks, oldest to newest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Requirement {
    pub min: u32,
    pub max: u32,
}

impl Requirement {
    /// What this binary speaks.
    pub fn own() -> Self {
        Self {
            min: MIN_PROTOCOL_VERSION,
            max: PROTOCOL_VERSION,
        }
    }

    /// The lines `archon update --requirements` prints; `install.sh` reads them.
    pub fn render(&self) -> String {
        format!("{MIN_KEY} {}\n{MAX_KEY} {}", self.min, self.max)
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let number = |key: &str| {
            text.lines()
                .find_map(|line| {
                    line.strip_prefix(key)?
                        .strip_prefix(' ')?
                        .parse::<u32>()
                        .ok()
                })
                .with_context(|| format!("no `{key} <number>` line"))
        };
        let (min, max) = (number(MIN_KEY)?, number(MAX_KEY)?);
        anyhow::ensure!(min <= max, "the protocol range {min}..{max} is empty");
        Ok(Self { min, max })
    }

    /// Ask the archon at `archon` what it speaks.
    pub async fn of(archon: &Path) -> anyhow::Result<Self> {
        let args = [OsStr::new("update"), OsStr::new("--requirements")];
        let output = tools::run(archon, &args, PROBE_TIMEOUT).await?;
        Self::parse(&String::from_utf8_lossy(&output.stdout))
            .with_context(|| format!("reading what {} needs", archon.display()))
    }
}

/// The protocols an installed aether serves, and its version line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Served {
    pub version: String,
    pub min: u32,
    pub max: u32,
}

#[derive(Deserialize)]
struct Schema {
    min_protocol_version: u32,
    protocol_version: u32,
}

/// What the `aether` on PATH serves; `None` when there is none.
pub async fn installed() -> anyhow::Result<Option<Served>> {
    let Some(aether) = find_program(Path::new(AETHER_PROGRAM)) else {
        return Ok(None);
    };
    let version = tools::run(&aether, &[OsStr::new("--version")], PROBE_TIMEOUT).await?;
    let schema = tools::run(&aether, &[OsStr::new("schema")], PROBE_TIMEOUT).await?;
    let schema: Schema = serde_json::from_slice(&schema.stdout)
        .with_context(|| format!("reading `{} schema`", aether.display()))?;
    anyhow::ensure!(
        schema.min_protocol_version <= schema.protocol_version,
        "`{} schema` names an empty protocol range",
        aether.display()
    );
    Ok(Some(Served {
        version: String::from_utf8_lossy(&version.stdout).trim().to_owned(),
        min: schema.min_protocol_version,
        max: schema.protocol_version,
    }))
}

/// Whether `served` and `needed` share a protocol, and one line saying so
/// for `archon` (a name and version), with the command to run when not.
pub fn verdict(archon: &str, needed: Requirement, served: Option<&Served>) -> (bool, String) {
    assert!(needed.min <= needed.max, "a requirement is a range");
    let needs = format!("{archon} needs protocol {} to {}", needed.min, needed.max);
    let Some(served) = served else {
        let line = format!("Aether: no `aether` on PATH; {needs}. Install Aether before use.");
        return (false, line);
    };
    let speaks = format!(
        "{} speaks protocol {} to {}",
        served.version, served.min, served.max
    );
    if served.max < needed.min {
        let line = format!("Aether: {speaks}, too old: {needs}. Run `aether update`.");
        return (false, line);
    }
    if served.min > needed.max {
        let line = format!("Aether: {speaks}, newer than {archon} supports: {needs}.");
        return (false, line);
    }
    (true, format!("Aether: {speaks}; {needs}: compatible."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served(min: u32, max: u32) -> Served {
        Served {
            version: "aether 0.9.0".to_owned(),
            min,
            max,
        }
    }

    #[test]
    fn a_requirement_survives_its_own_text_and_states_this_build() {
        let own = Requirement::own();
        assert_eq!((own.min, own.max), (MIN_PROTOCOL_VERSION, PROTOCOL_VERSION));
        assert_eq!(Requirement::parse(&own.render()).unwrap(), own);
        let padded = format!("note\n{}\nmore\n", own.render());
        assert_eq!(Requirement::parse(&padded).unwrap(), own);
        for text in [
            "",
            "aether_protocol_min 14",
            "aether_protocol_min x\naether_protocol_max 17",
            "aether_protocol_min 18\naether_protocol_max 17",
        ] {
            assert!(Requirement::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn an_aether_is_compatible_when_the_ranges_overlap() {
        let needed = Requirement { min: 14, max: 17 };
        for (min, max) in [(14, 17), (16, 17), (12, 14), (17, 19)] {
            let (ok, line) = verdict("archon 2.0.0", needed, Some(&served(min, max)));
            assert!(ok && line.ends_with("compatible."), "{min}..{max}: {line}");
        }
        let (ok, line) = verdict("archon 2.0.0", needed, Some(&served(12, 13)));
        assert!(!ok && line.ends_with("Run `aether update`."), "{line}");
        assert!(
            line.contains("aether 0.9.0 speaks protocol 12 to 13"),
            "{line}"
        );
        assert!(
            line.contains("archon 2.0.0 needs protocol 14 to 17"),
            "{line}"
        );
        let (ok, line) = verdict("archon 2.0.0", needed, Some(&served(18, 19)));
        assert!(
            !ok && line.contains("newer than archon 2.0.0 supports"),
            "{line}"
        );
        assert!(!line.contains("aether update"), "{line}");
        let (ok, line) = verdict("archon 2.0.0", needed, None);
        assert!(!ok && line.contains("no `aether` on PATH"), "{line}");
    }
}
