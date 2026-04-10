using System.Runtime.InteropServices;

namespace Polyplug.Abi {

/// <summary>
/// C-compatible string view for passing strings across the FFI boundary.
/// The pointer must remain valid for the duration of the call.
/// This is a borrowed view — the caller must NOT free the memory.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct StringView
{
    /// <summary>
    /// Pointer to UTF-8 bytes.
    /// </summary>
    public nint Ptr;

    /// <summary>
    /// Length in bytes.
    /// </summary>
    public nuint Len;
}

/// ABI error codes — returned by all ABI functions.
public enum AbiErrorCode : uint
{
    Ok = 0,
    Generic = 1,
    BufferTooSmall = 2,
    Panic = 3,
    NotFound = 4,
    StaleHandle = 5,
    FunctionNotAvailable = 6,
    DuplicateProvider = 7,
    InvalidPointer = 8,
    HostContractNotFound = 100,
    HostContractVersionMismatch = 101,
    HostContractCallFailed = 102,
}

/// ABI constants for polyplug.
public static class AbiConstants
{
    public const uint POLYPLUG_ABI_VERSION = 1u;
}
}
