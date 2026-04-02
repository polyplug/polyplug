/// ABI error codes (reserved: 0-255 runtime, 256+ plugin-defined).
///
/// These codes are returned by all ABI functions to indicate success or failure.
/// The `code` field of `AbiError` uses these values.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiErrorCode {
    /// Success — no error.
    Ok = 0,
    /// Generic error — unspecified failure.
    Generic = 1,
    /// Buffer too small — caller must reallocate (see Buffer protocol).
    BufferTooSmall = 2,
    /// Panic — plugin panicked (caught by catch_unwind).
    Panic = 3,
    /// Not found — plugin/contract not found.
    NotFound = 4,
    /// Stale handle — PluginHandle generation mismatch.
    StaleHandle = 5,
    /// Function not available — function_id >= function_count.
    FunctionNotAvailable = 6,
    /// Duplicate provider — same bundle already provides this contract.
    DuplicateProvider = 7,
    /// Invalid pointer — null or invalid pointer passed to ABI function.
    InvalidPointer = 8,
    // Host contract error codes (reserved: 100-199 host contracts)
    /// Host contract not found — no host contract matches contract_id.
    HostContractNotFound = 100,
    /// Host contract version mismatch — host contract version does not match.
    HostContractVersionMismatch = 101,
    /// Host contract call failed — host contract function call failed.
    HostContractCallFailed = 102,
}
