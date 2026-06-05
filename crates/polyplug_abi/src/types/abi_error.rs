use crate::types::{error_code::AbiErrorCode, string_view::StringView};

/// ABI error — returned by value from all ABI calls.
///
/// CODE: a raw `u32`, NOT the [`AbiErrorCode`] enum. Plugins are untrusted and
/// return `AbiError` by value across the C ABI, so any 32-bit pattern can land
/// here — including values that are not declared discriminants of the frozen
/// [`AbiErrorCode`] enum. Materializing such a value as the enum would be
/// instant undefined behaviour, so the field stays a raw `u32`. Construct it
/// with `AbiErrorCode::X as u32` and interpret it with
/// [`AbiErrorCode::from_u32`], which is total and safe. The layout is identical
/// to the `#[repr(u32)]` enum (4 bytes at offset 0), so the C ABI is unchanged.
///
/// OWNERSHIP: `message` is always a static or runtime-owned string. The receiver
/// must NEVER free it. Rich, allocated error detail is retrieved separately via
/// `get_last_error`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AbiError {
    /// 0 = success, non-zero = error. Raw `u32`; convert with
    /// [`AbiErrorCode::from_u32`].
    pub code: u32,
    /// Empty/NULL if success. UTF-8 message if non-zero code.
    pub message: StringView,
}

// SAFETY: AbiError contains a StringView which is Send+Sync, and a u32 code.
unsafe impl Send for AbiError {}

// SAFETY: AbiError contains a StringView which is Send+Sync (concurrent reads are safe), and a u32 code.
unsafe impl Sync for AbiError {}

impl AbiError {
    /// Construct a success AbiError.
    pub const fn ok() -> AbiError {
        AbiError {
            code: AbiErrorCode::Ok as u32,
            message: StringView::null(),
        }
    }

    /// Construct a panic error with a static message.
    pub const fn panic_caught() -> AbiError {
        AbiError {
            code: AbiErrorCode::Panic as u32,
            message: StringView::from_static(b"plugin panicked"),
        }
    }

    /// Returns true if this represents success.
    pub fn is_ok(&self) -> bool {
        self.code == AbiErrorCode::Ok as u32
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::types::abi_error::AbiError;

    #[test]
    fn layout_abi_error() {
        assert_eq!(size_of::<AbiError>(), 24);
        assert_eq!(align_of::<AbiError>(), 8);
        assert_eq!(offset_of!(AbiError, code), 0);
        assert_eq!(offset_of!(AbiError, message), 8);
    }
}
