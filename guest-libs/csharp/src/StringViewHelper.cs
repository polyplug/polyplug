using System.Text;

namespace Polyplug.Guest;

/// <summary>
/// Helpers for transcoding C# strings to UTF-8 StringViews at the ABI boundary.
/// Per AGENTS.md §9: all strings at the ABI boundary are UTF-8 StringView.
/// </summary>
public static unsafe class StringViewHelper
{
    /// <summary>
    /// Transcodes a C# string to UTF-8 in a host-allocated buffer.
    /// The returned StringView's memory is owned by the host allocator.
    /// Caller must free via host->Free(sv.Ptr, sv.Len, 1) when done.
    /// </summary>
    public static StringView FromString(string s, HostVTable* host)
    {
        int byteCount = Encoding.UTF8.GetByteCount(s);
        byte* buf = host->Alloc((nuint)byteCount, 1);
        if (buf == null)
        {
            throw new OutOfMemoryException("host_alloc returned null");
        }
        int written = Encoding.UTF8.GetBytes(s.AsSpan(), new Span<byte>(buf, byteCount));
        return new StringView { Ptr = buf, Len = (nuint)written };
    }

    /// <summary>
    /// Returns a StringView pointing to a static ASCII byte literal.
    /// Only safe for compile-time ASCII string literals pinned in a <c>fixed</c> block.
    /// The caller is responsible for ensuring ptr remains valid for the duration of the call.
    /// </summary>
    public static StringView FromStaticAscii(byte* ptr, int len)
    {
        return new StringView { Ptr = ptr, Len = (nuint)len };
    }
}
