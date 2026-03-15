//! Version — version struct, compatibility enum, and parse/compare logic.

use crate::error::LoaderError;
use core::fmt;

/// A two-component semantic version (major.minor).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl Version {
    /// Parse a version string of the form `"major.minor"`.
    ///
    /// Returns `Err(LoaderError::ManifestParse)` if the string is not exactly two
    /// dot-separated unsigned integers.
    pub fn parse(s: &str, context: &str) -> Result<Version, LoaderError> {
        let (major_str, minor_str): (&str, &str) =
            s.split_once('.')
                .ok_or_else(|| LoaderError::ManifestParse {
                    path: context.to_owned(),
                    reason: format!(
                        "invalid version string {:?}: expected \"major.minor\" format",
                        s
                    ),
                })?;

        let major: u32 = major_str
            .parse::<u32>()
            .map_err(|_| LoaderError::ManifestParse {
                path: context.to_owned(),
                reason: format!(
                    "invalid version string {:?}: expected \"major.minor\" format",
                    s
                ),
            })?;

        let minor: u32 = minor_str
            .parse::<u32>()
            .map_err(|_| LoaderError::ManifestParse {
                path: context.to_owned(),
                reason: format!(
                    "invalid version string {:?}: expected \"major.minor\" format",
                    s
                ),
            })?;

        Ok(Version { major, minor })
    }

    /// Returns `true` if `self` is compatible with the `required` version.
    ///
    /// Compatible means: same major version AND self.minor >= required.minor.
    pub fn is_compatible_with(&self, required: &Version) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

/// How strictly version compatibility is enforced when resolving plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compatibility {
    /// Exact major match and minor >= required.
    #[default]
    Strict,
    /// Same major, any minor.
    Relaxed,
    /// Any version accepted.
    Yolo,
}

#[cfg(test)]
mod tests {
    use super::{Compatibility, Version};

    #[test]
    fn version_parse_valid() {
        let v1: Version = Version::parse("1.0", "test").expect("parse 1.0");
        assert_eq!(v1, Version { major: 1, minor: 0 });

        let v2: Version = Version::parse("2.3", "test").expect("parse 2.3");
        assert_eq!(v2, Version { major: 2, minor: 3 });
    }

    #[test]
    fn version_parse_invalid() {
        assert!(Version::parse("1", "test").is_err());
        assert!(Version::parse("1.2.3", "test").is_err());
        assert!(Version::parse("", "test").is_err());
        assert!(Version::parse("not_a_version", "test").is_err());
    }

    #[test]
    fn version_compatible() {
        let v1_0 = Version { major: 1, minor: 0 };
        let v1_2 = Version { major: 1, minor: 2 };
        let v2_0 = Version { major: 2, minor: 0 };

        assert!(v1_2.is_compatible_with(&v1_0));
        assert!(!v1_0.is_compatible_with(&v1_2));
        assert!(!v2_0.is_compatible_with(&v1_0));
    }

    #[test]
    fn version_display() {
        let v: Version = Version { major: 1, minor: 2 };
        assert_eq!(v.to_string(), "1.2");
    }

    #[test]
    fn compatibility_default_is_strict() {
        assert_eq!(Compatibility::default(), Compatibility::Strict);
    }

    #[test]
    fn version_parse_four_component_overflow() {
        // "1.2.3.4" splits on first '.' giving minor_str = "2.3.4", which cannot
        // be parsed as u32 — must be rejected.
        assert!(Version::parse("1.2.3.4", "test").is_err());
    }

    #[test]
    fn version_parse_prerelease_rejected() {
        // Pre-release suffixes are not part of the "major.minor" format.
        // "1.0.0-alpha" splits to minor_str = "0.0-alpha", not a valid u32.
        assert!(Version::parse("1.0.0-alpha", "test").is_err());
        // "1.0.0-rc.1" splits to minor_str = "0.0-rc.1", not a valid u32.
        assert!(Version::parse("1.0.0-rc.1", "test").is_err());
    }

    #[test]
    fn version_parse_wildcard_requirements_rejected() {
        // Semver requirement strings must be rejected by the plain version parser.
        // "^1.2.0": major_str = "^1", not a valid u32.
        assert!(Version::parse("^1.2.0", "test").is_err());
        // "~1.2.0": major_str = "~1", not a valid u32.
        assert!(Version::parse("~1.2.0", "test").is_err());
        // ">=1.0": split_once gives major_str ">" which is not a valid u32.
        assert!(Version::parse(">=1.0", "test").is_err());
    }
}
