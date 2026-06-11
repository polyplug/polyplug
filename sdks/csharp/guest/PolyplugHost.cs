using System;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug.Abi;

namespace Polyplug.Guest;

/// <summary>
/// Guest-side access to the host runtime over the raw <see cref="HostApi"/>
/// function-pointer table. The host pointer flows from <c>create_instance</c>
/// through the author factory (<c>SetXxxFactory</c>) into each implementation —
/// NO process-wide host storage exists in this SDK, so every helper takes the
/// host pointer explicitly.
/// </summary>
public static unsafe class PolyplugHost
{
    /// <summary>
    /// Send a guest diagnostic to the host's logging funnel (<c>HostApi.Log</c>).
    /// </summary>
    /// <remarks>
    /// Routes to the same sink as <c>RuntimeConfig::log</c>: the host-installed
    /// callback when one is set, otherwise the host's stderr default (Error/Warn
    /// visibility only). The host receives <c>(level, scope, message)</c> verbatim
    /// and copies what it needs before returning — nothing here outlives the call.
    /// By convention <paramref name="scope"/> is <c>"guest.&lt;plugin-name&gt;"</c>.
    ///
    /// Both strings are transcoded UTF-16 → UTF-8 at the boundary (all ABI
    /// strings are UTF-8 <c>StringView</c>s) and the UTF-8 buffers are pinned
    /// with <c>fixed</c> for the duration of the call, so the GC cannot move
    /// them while the host reads through the raw pointers.
    ///
    /// No-op when <paramref name="hostPtr"/> is <see cref="IntPtr.Zero"/>, so
    /// plugins may call this unconditionally.
    /// </remarks>
    /// <param name="hostPtr">The <c>HostApi</c> pointer handed to the author factory.</param>
    /// <param name="level">Severity; the host clamps unknown values to <see cref="LogLevel.Error"/>.</param>
    /// <param name="scope">Short stable tag, e.g. <c>"guest.&lt;plugin-name&gt;"</c>.</param>
    /// <param name="message">The log message.</param>
    public static void Log(IntPtr hostPtr, LogLevel level, string scope, string message)
    {
        if (hostPtr == IntPtr.Zero)
            return;

        var host = (HostApi*)hostPtr;
        if (host->Log == IntPtr.Zero)
            return;

        byte[] scopeBytes = Encoding.UTF8.GetBytes(scope);
        byte[] messageBytes = Encoding.UTF8.GetBytes(message);
        var logFn = (delegate* unmanaged[Cdecl]<IntPtr, uint, StringView, StringView, void>)host->Log;

        // Pin both UTF-8 buffers for the call: StringView carries raw pointers
        // the GC cannot see, so the arrays must not move while the host reads
        // them. `fixed` over an empty array yields a null pointer — a null view
        // with Len 0, which the ABI documents as legal.
        fixed (byte* scopePtr = scopeBytes)
        fixed (byte* messagePtr = messageBytes)
        {
            var scopeView = new StringView { Ptr = (IntPtr)scopePtr, Len = (nuint)scopeBytes.Length };
            var messageView = new StringView { Ptr = (IntPtr)messagePtr, Len = (nuint)messageBytes.Length };
            logFn(hostPtr, (uint)level, scopeView, messageView);
        }
    }

    /// <summary>
    /// Allocate a <see cref="StringView"/> backed by host-owned memory.
    /// </summary>
    /// <remarks>
    /// The bytes are copied into a buffer allocated through the host allocator
    /// (<c>HostApi.Alloc</c>). Ownership transfers to the host: the returned
    /// view is meant to be handed back across the ABI as a function result, and
    /// the host frees it via <c>HostApi.Free</c>. The guest must NOT free it.
    /// Returns a null view if <paramref name="hostPtr"/> is null or allocation fails.
    /// </remarks>
    /// <param name="hostPtr">The <c>HostApi</c> pointer handed to the author factory.</param>
    /// <param name="value">The string to copy into host-owned memory.</param>
    public static StringView AllocString(IntPtr hostPtr, string value)
    {
        if (hostPtr == IntPtr.Zero)
            return new StringView { Ptr = IntPtr.Zero, Len = 0 };

        byte[] bytes = Encoding.UTF8.GetBytes(value);
        if (bytes.Length == 0)
            return new StringView { Ptr = IntPtr.Zero, Len = 0 };

        var host = (HostApi*)hostPtr;
        var allocFn = (delegate* unmanaged[Cdecl]<IntPtr, nuint, nuint, IntPtr>)host->Alloc;
        IntPtr buffer = allocFn(hostPtr, (nuint)bytes.Length, (nuint)1);
        if (buffer == IntPtr.Zero)
            return new StringView { Ptr = IntPtr.Zero, Len = 0 };

        Marshal.Copy(bytes, 0, buffer, bytes.Length);
        return new StringView { Ptr = buffer, Len = (nuint)bytes.Length };
    }
}
