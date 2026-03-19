using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Guest;

/// <summary>
/// Pins a managed string as UTF-8 bytes so a StringView can point at it safely.
/// Implements IDisposable — always use in a using statement.
/// </summary>
public sealed class PinnedStringView : IDisposable
{
    private GCHandle _handle;
    private readonly byte[] _bytes;

    public PinnedStringView(string value)
    {
        _bytes = Encoding.UTF8.GetBytes(value);
        _handle = GCHandle.Alloc(_bytes, GCHandleType.Pinned);
    }

    public StringView View
    {
        get => new StringView
        {
            Ptr = _handle.AddrOfPinnedObject(),
            Len = (ulong)_bytes.Length,
        };
    }

    public void Dispose()
    {
        if (_handle.IsAllocated)
            _handle.Free();
    }
}
