//! Version — version struct, and parse/compare logic.

use core::error::Error;
use core::fmt;
use core::str;
use core::str::FromStr;

#[repr(u32)]
#[derive(Debug, PartialEq, Eq)]
pub enum ParseVersionError {
    // The string was not one-to-three dot-separated components.
    // (Non-doc comment: keeps the generated SDK abi files byte-identical.)
    InvalidFormat = 0,
    // A component was present but not a valid unsigned 32-bit integer.
    InvalidInt = 1,
}

impl fmt::Display for ParseVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseVersionError::InvalidFormat => {
                f.write_str("invalid version format: expected \"major[.minor[.patch]]\"")
            }
            ParseVersionError::InvalidInt => {
                f.write_str("invalid version component: not an unsigned 32-bit integer")
            }
        }
    }
}

impl Error for ParseVersionError {}

/// A three-component semantic version (major.minor.patch).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Version {
    /// Returns `true` if `self` is compatible with the `required` version.
    ///
    /// Compatible means: same major version AND self.minor >= required.minor.
    pub fn is_compatible_with(&self, required: &Version) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

impl FromStr for Version {
    type Err = ParseVersionError;

    /// Parse a version string of the form `"major.minor[.patch]"`.
    ///
    /// Accepts one to three dot-separated unsigned 32-bit integers; an omitted patch
    /// defaults to `0`. Returns:
    /// - `Err(ParseVersionError::InvalidFormat)` if there are more than three
    ///   components, or a required leading component is missing, and
    /// - `Err(ParseVersionError::InvalidInt)` if any component is not a valid `u32`.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut v_iter: str::Split<'_, char> = value.split('.');

        let major: u32 = v_iter
            .next()
            .ok_or(ParseVersionError::InvalidFormat)?
            .parse::<u32>()
            .map_err(|_| ParseVersionError::InvalidInt)?;

        let minor: u32 = v_iter
            .next()
            .ok_or(ParseVersionError::InvalidFormat)?
            .parse::<u32>()
            .map_err(|_| ParseVersionError::InvalidInt)?;

        let patch: u32 = v_iter
            .next()
            .unwrap_or("0")
            .parse::<u32>()
            .map_err(|_| ParseVersionError::InvalidInt)?;

        // Reject extra components: a strict three-component format never carries a 4th+.
        if v_iter.next().is_some() {
            return Err(ParseVersionError::InvalidFormat);
        }

        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::Version;

    #[test]
    fn version_parse_valid() {
        let v1: Version = "1.0".parse::<Version>().expect("parse 1.0");
        assert_eq!(
            v1,
            Version {
                major: 1,
                minor: 0,
                patch: 0
            }
        );

        let v2: Version = "2.3.4".parse::<Version>().expect("parse 2.3.4");
        assert_eq!(
            v2,
            Version {
                major: 2,
                minor: 3,
                patch: 4,
            }
        );

        let v3: Version = "3.4.5".parse::<Version>().expect("parse 3.4.5");
        assert_eq!(
            v3,
            Version {
                major: 3,
                minor: 4,
                patch: 5,
            }
        );
    }

    #[test]
    fn version_parse_invalid() {
        assert!("1".parse::<Version>().is_err());
        assert!("1.".parse::<Version>().is_err());
        assert!("".parse::<Version>().is_err());
        assert!("not_a_version".parse::<Version>().is_err());
    }

    #[test]
    fn version_compatible() {
        let v1_0 = Version {
            major: 1,
            minor: 0,
            patch: 0,
        };
        let v1_2 = Version {
            major: 1,
            minor: 2,
            patch: 0,
        };
        let v2_0 = Version {
            major: 2,
            minor: 0,
            patch: 0,
        };

        assert!(v1_2.is_compatible_with(&v1_0));
        assert!(!v1_0.is_compatible_with(&v1_2));
        assert!(!v2_0.is_compatible_with(&v1_0));
    }

    #[test]
    fn version_display() {
        let v: Version = Version {
            major: 1,
            minor: 2,
            patch: 3,
        };
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn version_parse_four_component_rejected() {
        // "1.2.3.4" has a 4th component — the strict parser rejects it as InvalidFormat
        // rather than silently dropping the extra component.
        let err: super::ParseVersionError = "1.2.3.4"
            .parse::<Version>()
            .expect_err("1.2.3.4 must be rejected");
        assert_eq!(err, super::ParseVersionError::InvalidFormat);
    }

    #[test]
    fn parse_version_error_display() {
        assert_eq!(
            super::ParseVersionError::InvalidFormat.to_string(),
            "invalid version format: expected \"major[.minor[.patch]]\""
        );
        assert_eq!(
            super::ParseVersionError::InvalidInt.to_string(),
            "invalid version component: not an unsigned 32-bit integer"
        );
    }

    #[test]
    fn version_parse_prerelease_rejected() {
        // Pre-release suffixes are not part of the "major.minor" format.
        // "1.0.0-alpha" splits to minor_str = "0.0-alpha", not a valid u32.
        assert!("1.0.0-alpha".parse::<Version>().is_err());
        // "1.0.0-rc.1" splits to minor_str = "0.0-rc.1", not a valid u32.
        assert!("1.0.0-rc.1".parse::<Version>().is_err());
    }

    #[test]
    fn version_parse_wildcard_requirements_rejected() {
        // Semver requirement strings must be rejected by the plain version parser.
        // "^1.2.0": major_str = "^1", not a valid u32.
        assert!("^1.2.0".parse::<Version>().is_err());
        // "~1.2.0": major_str = "~1", not a valid u32.
        assert!("~1.2.0".parse::<Version>().is_err());
        // ">=1.0": split_once gives major_str ">" which is not a valid u32.
        assert!(">=1.0".parse::<Version>().is_err());
    }
}
