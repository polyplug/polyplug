using System.Runtime.InteropServices;
using System.Text;

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

    /// <summary>
    /// Creates a StringView from a .NET string by pinning it in memory.
    /// The GCHandle must be kept alive while the StringView is in use.
    /// For guest plugins, return strings should use host allocation via registrar.
    /// </summary>
    public static (StringView, GCHandle) FromStringPinned(string str)
    {
        if (string.IsNullOrEmpty(str))
            return (new StringView { Ptr = IntPtr.Zero, Len = 0 }, default);

        byte[] bytes = Encoding.UTF8.GetBytes(str);
        GCHandle handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        StringView sv = new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (ulong)bytes.Length };
        return (sv, handle);
    }

    /// <summary>
    /// Converts a StringView to a .NET string by copying the UTF-8 bytes.
    /// </summary>
    public static string ToString(this StringView sv)
    {
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return string.Empty;

        byte[] bytes = new byte[(int)sv.Len];
        Marshal.Copy(sv.Ptr, bytes, 0, (int)sv.Len);
        return Encoding.UTF8.GetString(bytes);
    }
}
