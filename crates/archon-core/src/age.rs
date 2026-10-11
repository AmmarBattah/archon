//! Ages written like `90s`, `30m`, `12h`, `7d` or `2w`: `archon gc
//! --older-than` and the `[worktrees]` gc settings.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ArchonError;

/// Seconds per unit, largest last, so an age is shown in the largest unit
/// that divides it.
const UNITS: [(&str, u64); 5] = [
    ("s", 1),
    ("m", 60),
    ("h", 3_600),
    ("d", 86_400),
    ("w", 604_800),
];

/// A span of whole seconds, kept in config files as text like `7d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Age(Duration);

impl Age {
    pub const fn from_secs(secs: u64) -> Self {
        Self(Duration::from_secs(secs))
    }

    pub fn duration(self) -> Duration {
        self.0
    }

    pub fn secs(self) -> u64 {
        self.0.as_secs()
    }

    /// `self` as a `chrono` span, for arithmetic on timestamps.
    pub fn chrono(self) -> Result<chrono::Duration, ArchonError> {
        chrono::Duration::from_std(self.0)
            .map_err(|_| ArchonError::Invalid(format!("the age {self} is too large")))
    }

    /// `text` as a number and a unit: `90s`, `30m`, `12h`, `7d` or `2w`.
    pub fn parse(text: &str) -> Result<Self, ArchonError> {
        let text = text.trim();
        let split = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (digits, unit) = text.split_at(split);
        let amount: u64 = digits.parse().map_err(|_| {
            ArchonError::Invalid(format!("{text:?} is not an age like 7d, 12h or 30m"))
        })?;
        let Some((_, seconds)) = UNITS.iter().find(|(name, _)| *name == unit) else {
            return Err(ArchonError::Invalid(format!(
                "{text:?} needs a unit: s, m, h, d or w (e.g. 7d)"
            )));
        };
        let total = amount
            .checked_mul(*seconds)
            .filter(|total| {
                i64::try_from(*total).is_ok_and(|s| chrono::Duration::try_seconds(s).is_some())
            })
            .ok_or_else(|| ArchonError::Invalid(format!("{text:?} is too long")))?;
        let age = Self::from_secs(total);
        debug_assert!(age.chrono().is_ok(), "checked above");
        Ok(age)
    }
}

impl fmt::Display for Age {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.secs();
        if secs == 0 {
            return f.write_str("0s");
        }
        let (unit, per) = UNITS
            .iter()
            .rev()
            .find(|(_, per)| secs.is_multiple_of(*per))
            .expect("every age is whole seconds");
        write!(f, "{}{unit}", secs / per)
    }
}

impl Serialize for Age {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Age {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_need_a_number_and_a_unit() {
        assert_eq!(Age::parse("7d").unwrap().secs(), 7 * 86_400);
        assert_eq!(Age::parse("90s").unwrap().secs(), 90);
        assert_eq!(Age::parse(" 2w ").unwrap().secs(), 14 * 86_400);
        assert_eq!(Age::parse("0s").unwrap().secs(), 0);
        for bad in [
            "7",
            "d",
            "7y",
            "-1d",
            "99999999999999999999d",
            "9999999999999w",
        ] {
            assert!(Age::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ages_show_in_the_largest_unit_that_divides_them_and_round_trip() {
        for (text, shown) in [("60m", "1h"), ("90s", "90s"), ("14d", "2w"), ("0s", "0s")] {
            let age = Age::parse(text).unwrap();
            assert_eq!(age.to_string(), shown);
            assert_eq!(Age::parse(shown).unwrap(), age);
        }
        let json = serde_json::to_string(&Age::from_secs(3_600)).unwrap();
        assert_eq!(json, "\"1h\"");
        assert_eq!(serde_json::from_str::<Age>(&json).unwrap().secs(), 3_600);
        assert!(serde_json::from_str::<Age>("\"soon\"").is_err());
    }
}
