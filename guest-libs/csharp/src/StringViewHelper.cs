using System.Runtime.InteropServices;

namespace Polyplug.Guest;

/// <summary>
/// Helpers for constructing StringViews at the ABI boundary.
/// </summary>
public static class StringViewHelper
{
    /// <summary>
    /// Returns a StringView pointing at the pinned byte array via a GCHandle.
    /// Caller owns the GCHandle and must keep it alive while the StringView is in use.
    /// </summary>
    public static StringView FromPinnedHandle(GCHandle handle, int length) =>
        new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (ulong)length };

    /// <summary>
    /// Returns a StringView pointing at a pre-pinned IntPtr. Caller ensures ptr validity.
    /// </summary>
    public static StringView FromPtr(IntPtr ptr, int length) =>
        new StringView { Ptr = ptr, Len = (ulong)length };
}
