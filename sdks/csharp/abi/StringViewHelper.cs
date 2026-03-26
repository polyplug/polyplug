using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Abi;

/// <summary>
/// Helpers for constructing and converting StringViews at the ABI boundary.
/// This is the unified implementation used by both host and guest.
/// </summary>
public static class StringViewHelper
{
    /// <summary>
    /// Returns a StringView pointing at the pinned byte array via a GCHandle.
    /// Caller owns the GCHandle and must keep it alive while the StringView is in use.
    /// </summary>
    public static StringView FromPinnedHandle(GCHandle handle, int length) =>
        new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (nuint)length };

    /// <summary>
    /// Returns a StringView pointing at a pre-pinned IntPtr. Caller ensures ptr validity.
    /// </summary>
    public static StringView FromPtr(IntPtr ptr, int length) =>
        new StringView { Ptr = ptr, Len = (nuint)length };

    /// <summary>
    /// Creates a StringView from a .NET string by pinning it in memory.
    /// The GCHandle must be kept alive while the StringView is in use.
    /// For guest plugins, return strings should use host allocation via registrar.
    /// </summary>
    public static (StringView View, GCHandle Handle) FromStringPinned(string str)
    {
        if (string.IsNullOrEmpty(str))
            return (new StringView { Ptr = IntPtr.Zero, Len = 0 }, default);

        byte[] bytes = Encoding.UTF8.GetBytes(str);
        GCHandle handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        StringView sv = new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (nuint)bytes.Length };
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

    /// <summary>
    /// Converts a StringView to a .NET string. Alias for ToString.
    /// </summary>
    public static string ToStr(StringView sv) => ToString(sv);

    /// <summary>
    /// Checks if a StringView starts with the given prefix.
    /// </summary>
    public static bool StartsWith(StringView sv, string prefix)
    {
        if (string.IsNullOrEmpty(prefix))
            return true;
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return false;

        string str = ToString(sv);
        return str.StartsWith(prefix);
    }

    /// <summary>
    /// Checks if a StringView ends with the given suffix.
    /// </summary>
    public static bool EndsWith(StringView sv, string suffix)
    {
        if (string.IsNullOrEmpty(suffix))
            return true;
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return false;

        string str = ToString(sv);
        return str.EndsWith(suffix);
    }

    /// <summary>
    /// Strips the prefix from a StringView if it starts with it.
    /// Returns the original string if the prefix is not present.
    /// </summary>
    public static string StripPrefix(StringView sv, string prefix)
    {
        if (string.IsNullOrEmpty(prefix))
            return ToString(sv);

        string str = ToString(sv);
        if (str.StartsWith(prefix))
            return str.Substring(prefix.Length);
        return str;
    }

    /// <summary>
    /// Splits a StringView by the given delimiter and returns an array of strings.
    /// </summary>
    public static string[] Split(StringView sv, string delimiter)
    {
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return System.Array.Empty<string>();

        string str = ToString(sv);
        if (string.IsNullOrEmpty(delimiter))
            return new[] { str };

        return str.Split(new[] { delimiter }, System.StringSplitOptions.None);
    }
}