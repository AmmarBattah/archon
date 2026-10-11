//! Release versions as tags carry them: `v1.2.3`, or a pre-release such as
//! `v1.2.3-rc.1`, ordered as Semantic Versioning orders them.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    /// Empty for a release; a pre-release sorts before its release.
    pre: Vec<Identifier>,
}

/// Declared in precedence order: a number sorts before any text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Number(u64),
    Text(String),
}

impl Version {
    /// The version of this binary.
    pub fn current() -> Self {
        env!("CARGO_PKG_VERSION")
            .parse()
            .expect("the workspace version is a release version")
    }

    /// The release tag, e.g. `v1.2.3`.
    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

impl FromStr for Version {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        let invalid = || anyhow::anyhow!("`{text}` is not a version like v1.2.3");
        let bare = text.strip_prefix('v').unwrap_or(text);
        let (core, pre) = match bare.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (bare, None),
        };
        let numbers: Vec<u64> = core
            .split('.')
            .map(number)
            .collect::<Option<_>>()
            .ok_or_else(invalid)?;
        let core: [u64; 3] = numbers.try_into().map_err(|_| invalid())?;
        let pre = match pre {
            Some(pre) => pre
                .split('.')
                .map(identifier)
                .collect::<Option<_>>()
                .ok_or_else(invalid)?,
            None => Vec::new(),
        };
        Ok(Self { core, pre })
    }
}

/// A run of ASCII digits; `u64::from_str` alone would accept `+1`.
fn number(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| text.parse().ok()).flatten()
}

fn identifier(text: &str) -> Option<Identifier> {
    let allowed = |b: u8| b.is_ascii_alphanumeric() || b == b'-';
    if text.is_empty() || !text.bytes().all(allowed) {
        return None;
    }
    Some(number(text).map_or_else(|| Identifier::Text(text.to_owned()), Identifier::Number))
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.core;
        write!(f, "{major}.{minor}.{patch}")?;
        for (index, part) in self.pre.iter().enumerate() {
            f.write_str(if index == 0 { "-" } else { "." })?;
            match part {
                Identifier::Number(n) => write!(f, "{n}")?,
                Identifier::Text(text) => f.write_str(text)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn versions_parse_with_or_without_the_tag_prefix() {
        assert_eq!(version("v1.2.3"), version("1.2.3"));
        assert_eq!(version("v1.2.3").to_string(), "1.2.3");
        assert_eq!(version("1.2.3").tag(), "v1.2.3");
        assert_eq!(version("v2.0.0-rc.1").tag(), "v2.0.0-rc.1");
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn anything_else_is_refused_by_name() {
        for text in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "v1.2.-3",
            "+1.2.3",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-a_b",
            "1.2.3+build",
            "latest",
            "v1.2.3 ",
            "../1.2.3",
        ] {
            let error = text.parse::<Version>().unwrap_err().to_string();
            assert!(
                error.contains("not a version like v1.2.3"),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn versions_order_numerically_not_as_text() {
        assert!(version("1.10.0") > version("1.9.9"));
        assert!(version("2.0.0") > version("1.99.99"));
        assert!(version("1.0.1") > version("1.0.0"));
        assert_eq!(version("v1.0.0").cmp(&version("1.0.0")), Ordering::Equal);
    }

    #[test]
    fn a_pre_release_sorts_before_its_release_and_by_its_parts() {
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1-rc.1",
        ];
        for pair in ordered.windows(2) {
            assert!(version(pair[0]) < version(pair[1]), "{pair:?}");
        }
    }
}
