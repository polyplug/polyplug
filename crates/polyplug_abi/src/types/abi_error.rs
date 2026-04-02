use crate::types::{error_code::AbiErrorCode, string_view::StringView};

/// ABI error — returned by value from all ABI calls.
///
/// OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
/// via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
/// after reading. If `code == AbiErrorCode::Ok`, `message.ptr` is NULL — no free needed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AbiError {
    /// 0 = success, non-zero = error.
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
    pub const fn is_ok(&self) -> bool {
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
