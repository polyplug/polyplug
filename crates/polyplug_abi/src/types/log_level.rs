/// Log severity levels for the host-supplied logger callback (`RuntimeConfig::log`).
///
/// Numeric ordering is significant: lower values are more severe. A message is
/// delivered to the callback only when `level as u32 <= RuntimeConfig::log_max_level`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Unrecoverable or host-visible failures (e.g. rejected registrations).
    Error = 1,
    /// Recoverable anomalies the host should know about (e.g. lock poison recovery).
    Warn = 2,
    /// High-level lifecycle events.
    Info = 3,
    /// Detailed diagnostic information.
    Debug = 4,
    /// Very fine-grained tracing.
    Trace = 5,
}

impl core::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "Error"),
            LogLevel::Warn => write!(f, "Warn"),
            LogLevel::Info => write!(f, "Info"),
            LogLevel::Debug => write!(f, "Debug"),
            LogLevel::Trace => write!(f, "Trace"),
        }
    }
}

impl LogLevel {
    /// Checked conversion from a raw `u32` arriving across the C ABI.
    ///
    /// Any `u32` bit pattern can cross the boundary; values that are not declared
    /// discriminants of this enum yield `None`. No `unsafe`, no transmute: an
    /// arbitrary `u32` is never reinterpreted as an enum discriminant.
    #[inline]
    pub const fn from_u32(level: u32) -> Option<LogLevel> {
        match level {
            1 => Some(LogLevel::Error),
            2 => Some(LogLevel::Warn),
            3 => Some(LogLevel::Info),
            4 => Some(LogLevel::Debug),
            5 => Some(LogLevel::Trace),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::types::log_level::LogLevel;

    #[test]
    fn repr_values_are_1_through_5() {
        assert_eq!(LogLevel::Error as u32, 1);
        assert_eq!(LogLevel::Warn as u32, 2);
        assert_eq!(LogLevel::Info as u32, 3);
        assert_eq!(LogLevel::Debug as u32, 4);
        assert_eq!(LogLevel::Trace as u32, 5);
    }

    #[test]
    fn from_u32_maps_known_levels() {
        assert_eq!(LogLevel::from_u32(1), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_u32(2), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_u32(3), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_u32(4), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_u32(5), Some(LogLevel::Trace));
    }

    #[test]
    fn from_u32_rejects_unknown_levels() {
        assert_eq!(LogLevel::from_u32(0), None);
        assert_eq!(LogLevel::from_u32(6), None);
        assert_eq!(LogLevel::from_u32(u32::MAX), None);
    }
}
