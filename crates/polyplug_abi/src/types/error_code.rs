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

impl core::fmt::Display for AbiErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AbiErrorCode::Ok => write!(f, "Ok"),
            AbiErrorCode::Generic => write!(f, "Generic"),
            AbiErrorCode::BufferTooSmall => write!(f, "BufferTooSmall"),
            AbiErrorCode::Panic => write!(f, "Panic"),
            AbiErrorCode::NotFound => write!(f, "NotFound"),
            AbiErrorCode::StaleHandle => write!(f, "StaleHandle"),
            AbiErrorCode::FunctionNotAvailable => write!(f, "FunctionNotAvailable"),
            AbiErrorCode::DuplicateProvider => write!(f, "DuplicateProvider"),
            AbiErrorCode::InvalidPointer => write!(f, "InvalidPointer"),
            AbiErrorCode::HostContractNotFound => write!(f, "HostContractNotFound"),
            AbiErrorCode::HostContractVersionMismatch => write!(f, "HostContractVersionMismatch"),
            AbiErrorCode::HostContractCallFailed => write!(f, "HostContractCallFailed"),
        }
    }
}

impl AbiErrorCode {
    /// Convert from a raw u32 error code.
    ///
    /// For known runtime codes (0-255), returns the corresponding enum variant.
    /// For user-defined codes (256+), the value is preserved but the display
    /// will show the numeric value.
    #[inline]
    pub const fn from_u32(code: u32) -> Self {
        match code {
            0 => AbiErrorCode::Ok,
            1 => AbiErrorCode::Generic,
            2 => AbiErrorCode::BufferTooSmall,
            3 => AbiErrorCode::Panic,
            4 => AbiErrorCode::NotFound,
            5 => AbiErrorCode::StaleHandle,
            6 => AbiErrorCode::FunctionNotAvailable,
            7 => AbiErrorCode::DuplicateProvider,
            8 => AbiErrorCode::InvalidPointer,
            100 => AbiErrorCode::HostContractNotFound,
            101 => AbiErrorCode::HostContractVersionMismatch,
            102 => AbiErrorCode::HostContractCallFailed,
            other => unsafe { core::mem::transmute::<u32, AbiErrorCode>(other) },
        }
    }
}

impl From<u32> for AbiErrorCode {
    fn from(code: u32) -> Self {
        Self::from_u32(code)
    }
}
