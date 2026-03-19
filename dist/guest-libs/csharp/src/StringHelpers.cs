using System;
using System.Text;

namespace PolyplugGuest;

/// <summary>
/// String conversion helpers for guest plugins.
/// </summary>
public static class StringHelpers
{
    /// <summary>
    /// Convert StringView to C# string.
    /// </summary>
    /// <param name="sv">StringView from polyplug ABI</param>
    /// <returns>C# string (UTF-8 decoded)</returns>
    public static string ToString(StringView sv)
    {
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return string.Empty;
        
        var bytes = new byte[sv.Len];
        System.Runtime.InteropServices.Marshal.Copy(sv.Ptr, bytes, 0, sv.Len);
        return Encoding.UTF8.GetString(bytes);
    }

    /// <summary>
    /// Allocate StringView from C# string using host allocator.
    /// </summary>
    /// <param name="s">C# string to convert</param>
    /// <returns>StringView pointing to host-allocated memory</returns>
    public static StringView AllocString(string s)
    {
        var bytes = Encoding.UTF8.GetBytes(s);
        var ptr = Abi.HostAlloc(bytes.Length, 1);
        System.Runtime.InteropServices.Marshal.Copy(bytes, 0, ptr, bytes.Length);
        return new StringView { Ptr = ptr, Len = bytes.Length };
    }
}
