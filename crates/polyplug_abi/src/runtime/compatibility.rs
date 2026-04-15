//! Compatibility — version compatibility enforcement modes.

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

    #[test]
    fn compatibility_repr_u32() {
        assert_eq!(Compatibility::Strict as u32, 0);
        assert_eq!(Compatibility::Relaxed as u32, 1);
        assert_eq!(Compatibility::Yolo as u32, 2);
    }
}
