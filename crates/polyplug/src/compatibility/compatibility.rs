//! Compatibility — compatibility enum.

/// How strictly version compatibility is enforced when resolving plugins.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compatibility {
    /// Exact major match and minor >= required.
    #[default]
    Strict = 0,
    /// Same major, any minor.
    Relaxed = 1,
    /// Any version accepted.
    Yolo = 2,
}

#[cfg(test)]
mod tests {
    use super::Compatibility;

    #[test]
    fn compatibility_default_is_strict() {
        assert_eq!(Compatibility::default(), Compatibility::Strict);
    }
}
