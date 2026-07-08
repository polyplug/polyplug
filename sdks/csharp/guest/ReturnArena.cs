using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

using Polyplug.Abi;

namespace Polyplug.Guest;

/// <summary>
/// Guest-owned bump arena for building a variable-size return whose parts must
/// outlive the call that produced them — a <see cref="StringView"/>, or an
/// <c>ArrayOf_T</c> wrapper (from <c>Array&lt;T&gt;</c>) plus any strings its
/// elements embed.
///
/// <para>The native ABI dispatch hands the guest a caller-owned <c>out</c> slot
/// sized for the fixed return struct only; the wrapper it writes there points at
/// the element array and each string, which therefore must live in memory the
/// guest owns. A plugin allocates every string with <see cref="AllocString"/>,
/// builds its element array with those views, allocates the array with
/// <see cref="AllocArray{T}"/>, and returns the raw wrapper pointing into the
/// arena. Every view stays valid until the next <see cref="Reset"/> — the
/// borrowed-return contract, identical to the Rust guest SDK's ReturnArena.</para>
///
/// <para>Not thread-safe: if one instance is shared across threads, guard it with
/// your own lock (the Rust reference wraps it in a Mutex).</para>
/// </summary>
public sealed unsafe class ReturnArena : IDisposable
{
    private byte* _buf;
    private readonly nuint _capacity;
    private nuint _cursor;

    /// <summary>
    /// Allocate a guest-owned, off-GC-heap buffer of <paramref name="capacity"/>
    /// bytes. Size it for the largest return a method builds; overflow throws
    /// <see cref="GuestException"/> with <see cref="AbiErrorCode.BufferTooSmall"/>.
    /// </summary>
    public ReturnArena(nuint capacity)
    {
        _buf = (byte*)NativeMemory.Alloc(capacity);
        _capacity = capacity;
        _cursor = 0;
    }

    /// <summary>
    /// Rewind the arena to empty. Every view previously returned by this arena is
    /// now dangling — call once at the start of each arena-backed method, before
    /// allocating that call's return.
    /// </summary>
    public void Reset() => _cursor = 0;

    /// <summary>Bump-allocate <paramref name="size"/> bytes aligned to
    /// <paramref name="align"/> (a power of two). Throws on overflow.</summary>
    private byte* Bump(nuint size, nuint align)
    {
        nuint aligned = (_cursor + (align - 1)) & ~(align - 1);
        // Split the bound so the subtraction can never underflow.
        if (aligned > _capacity || size > _capacity - aligned)
        {
            throw new GuestException(
                (uint)AbiErrorCode.BufferTooSmall,
                "ReturnArena capacity exceeded");
        }
        byte* p = _buf + aligned;
        _cursor = aligned + size;
        return p;
    }

    /// <summary>
    /// Copy <paramref name="value"/> as UTF-8 into the arena and return a
    /// <see cref="StringView"/> over the bytes. A null or empty string yields a
    /// null view (no allocation), matching the ABI's "null view == empty" rule.
    /// </summary>
    public StringView AllocString(string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return new StringView { Ptr = IntPtr.Zero, Len = 0 };
        }

        int byteCount = Encoding.UTF8.GetByteCount(value);
        byte* dst = Bump((nuint)byteCount, 1);
        Encoding.UTF8.GetBytes(value, new Span<byte>(dst, byteCount));
        return new StringView { Ptr = (IntPtr)dst, Len = (nuint)byteCount };
    }

    /// <summary>
    /// Copy <paramref name="elements"/> into the arena and return
    /// <c>(items, len)</c> for an <c>ArrayOf_T</c> wrapper: <c>items</c> is the
    /// element base address, <c>len</c> the element count. Element structs may
    /// embed <see cref="StringView"/>s previously produced by
    /// <see cref="AllocString"/> on this same arena — build the element array with
    /// those views, then allocate it here. An empty span yields <c>(0, 0)</c>.
    /// </summary>
    public (ulong items, ulong len) AllocArray<T>(ReadOnlySpan<T> elements)
        where T : unmanaged
    {
        if (elements.Length == 0)
        {
            return (0UL, 0UL);
        }

        nuint elemSize = (nuint)Unsafe.SizeOf<T>();
        // Align to 8 — the maximum natural alignment of any polyplug ABI POD
        // (u64 / f64 / pointer). NativeMemory.Alloc returns at least pointer-
        // aligned memory, so an 8-aligned offset yields an 8-aligned element base.
        byte* dst = Bump(elemSize * (nuint)elements.Length, 8);
        elements.CopyTo(new Span<T>(dst, elements.Length));
        return ((ulong)dst, (ulong)elements.Length);
    }

    /// <summary>Release the guest-owned buffer. After disposal every view the
    /// arena returned is invalid.</summary>
    public void Dispose()
    {
        FreeBuffer();
        GC.SuppressFinalize(this);
    }

    ~ReturnArena() => FreeBuffer();

    private void FreeBuffer()
    {
        if (_buf != null)
        {
            NativeMemory.Free(_buf);
            _buf = null;
        }
    }
}
