// Unit tests for ReturnArena — the guest-side bump arena that builds
// variable-size returns (a StringView, or an ArrayOf_T plus the strings its
// elements embed) into guest-owned memory, valid until the next Reset().
//
// These exercise the arena in isolation; the end-to-end round trip through a
// generated guest contract lives in polyplugc's generate_e2e_array suite
// (csharp_kitchen_all_widths_round_trips).

using System.Runtime.InteropServices;
using System.Text;

using Polyplug.Abi;
using Xunit;

namespace Polyplug.Guest.Tests;

public sealed unsafe class ReturnArenaTests
{
    private static string Read(StringView v) =>
        v.Ptr == IntPtr.Zero || v.Len == 0 ? "" : Encoding.UTF8.GetString((byte*)v.Ptr, (int)v.Len);

    [Fact]
    public void AllocString_IsByteExact_IncludingUnicode()
    {
        using var arena = new ReturnArena(256);
        StringView v = arena.AllocString("café.exe");
        Assert.Equal((nuint)9, v.Len); // 'é' encodes to two UTF-8 bytes → 9 total.
        Assert.Equal("café.exe", Read(v));
    }

    [Fact]
    public void AllocString_NullOrEmpty_YieldsNullView()
    {
        using var arena = new ReturnArena(64);
        StringView empty = arena.AllocString("");
        Assert.Equal(IntPtr.Zero, empty.Ptr);
        Assert.Equal((nuint)0, empty.Len);
        StringView nul = arena.AllocString(null);
        Assert.Equal(IntPtr.Zero, nul.Ptr);
    }

    [Fact]
    public void AllocArray_CopiesElements_AndEmbeddedStringsSurvive()
    {
        using var arena = new ReturnArena(256);
        StringView a = arena.AllocString("one");
        StringView b = arena.AllocString("two");
        Row[] rows = { new() { N = 1, S = a }, new() { N = 2, S = b } };
        (ulong items, ulong len) = arena.AllocArray<Row>(rows);

        Assert.Equal(2UL, len);
        Row* p = (Row*)(void*)(nuint)items;
        Assert.Equal(1, p[0].N);
        Assert.Equal("one", Read(p[0].S));
        Assert.Equal(2, p[1].N);
        Assert.Equal("two", Read(p[1].S));
    }

    [Fact]
    public void AllocArray_Empty_YieldsZeroItemsZeroLen()
    {
        using var arena = new ReturnArena(64);
        (ulong items, ulong len) = arena.AllocArray<Row>(ReadOnlySpan<Row>.Empty);
        Assert.Equal(0UL, items);
        Assert.Equal(0UL, len);
    }

    [Fact]
    public void AllocArray_BaseIs8Aligned_AfterOddLengthStringAlloc()
    {
        using var arena = new ReturnArena(256);
        arena.AllocString("abc"); // 3 bytes → leaves the cursor at an odd offset.
        (ulong items, ulong _) = arena.AllocArray<Row>(new Row[] { new() { N = 7 } });
        Assert.Equal(0UL, items % 8);
    }

    [Fact]
    public void Reset_RewindsBuffer_ForReuse()
    {
        // Capacity only fits one "reused" at a time; 100 iterations prove Reset
        // rewinds rather than leaking the cursor forward.
        using var arena = new ReturnArena(16);
        for (int i = 0; i < 100; i++)
        {
            arena.Reset();
            Assert.Equal("reused", Read(arena.AllocString("reused")));
        }
    }

    [Fact]
    public void Overflow_Throws_BufferTooSmall()
    {
        using var arena = new ReturnArena(4);
        GuestException ex = Assert.Throws<GuestException>(
            () => arena.AllocString("too long for four bytes"));
        Assert.Equal((uint)AbiErrorCode.BufferTooSmall, ex.Code);
    }
}

[StructLayout(LayoutKind.Sequential)]
internal struct Row
{
    public int N;
    public StringView S;
}
