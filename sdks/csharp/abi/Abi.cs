using System.Runtime.InteropServices;

namespace Polyplug.Abi {


/// ABI constants for polyplug.
public static class AbiConstants
{
    public const uint ABI_OK = 0u;
    public const uint ABI_ERROR_GENERIC = 1u;
    public const uint ABI_ERROR_BUFFER_TOO_SMALL = 2u;
    public const uint ABI_ERROR_PANIC = 3u;
    public const uint ABI_ERROR_NOT_FOUND = 4u;
    public const uint ABI_ERROR_STALE_HANDLE = 5u;
    public const uint ABI_ERROR_FUNCTION_NOT_AVAILABLE = 6u;
    public const uint ABI_ERROR_DUPLICATE_PROVIDER = 7u;
    public const uint ABI_ERROR_INVALID_POINTER = 8u;
    public const uint ABI_HOST_CONTRACT_NOT_FOUND = 100u;
    public const uint ABI_HOST_CONTRACT_VERSION_MISMATCH = 101u;
    public const uint ABI_HOST_CONTRACT_CALL_FAILED = 102u;
    public const uint POLYPLUG_ABI_VERSION = 1u;
}
}
