mod abi_error;
mod array;
mod buffer;
mod dependency_info;
mod error_code;
mod string_view;
mod version;

pub use abi_error::AbiError;
pub use array::Array;
pub use buffer::Buffer;
pub use dependency_info::DependencyInfo;
pub use error_code::AbiErrorCode;
pub use string_view::StringView;
pub use version::{Version, ParseVersionError};

// ─── FFI Helper Functions ───────────────────────────────────────────────────────

/// Create an AbiError success response.
/// Convenience function for FFI boundary code.
pub const fn abi_error_ok() -> AbiError {
    AbiError::ok()
}

/// Create a null/empty StringView.
/// Convenience function for FFI boundary code.
pub const fn string_view_null() -> StringView {
    StringView::null()
}

/// Create a StringView from static bytes.
/// Convenience function for FFI boundary code.
pub const fn string_view_from_static(bytes: &'static [u8]) -> StringView {
    StringView::from_static(bytes)
}