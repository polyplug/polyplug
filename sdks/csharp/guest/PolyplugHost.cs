using System;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug.Abi;

namespace Polyplug.Guest;

/// <summary>
/// Guest-side access to the host runtime stored during <c>polyplug_init</c>.
/// Provides the host allocator and extension lookup over the raw
/// <see cref="HostApi"/> function-pointer table.
/// </summary>
public static unsafe class PolyplugHost
{
    /// <summary>
    /// Allocate a <see cref="StringView"/> backed by host-owned memory.
    /// </summary>
    /// <remarks>
    /// The bytes are copied into a buffer allocated through the host allocator
    /// (<c>HostApi.Alloc</c>). Ownership transfers to the host: the returned
    /// view is meant to be handed back across the ABI as a function result, and
    /// the host frees it via <c>HostApi.Free</c>. The guest must NOT free it.
    /// Returns a null view if no host is stored or allocation fails.
    /// </remarks>
    public static StringView AllocString(string value)
    {
        IntPtr hostPtr = RuntimeAbiStorage.GetRuntimeAbi();
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

    /// <summary>
    /// Look up a host extension by its pre-computed FNV-1a-32 identifier.
    /// </summary>
    /// <returns>
    /// The opaque extension pointer, or <see cref="IntPtr.Zero"/> if no host is
    /// stored or the extension is not registered.
    /// </returns>
    public static IntPtr GetExtension(uint extensionId)
    {
        IntPtr hostPtr = RuntimeAbiStorage.GetRuntimeAbi();
        if (hostPtr == IntPtr.Zero)
            return IntPtr.Zero;

        var host = (HostApi*)hostPtr;
        var getExtensionFn = (delegate* unmanaged[Cdecl]<IntPtr, uint, IntPtr>)host->GetExtension;
        return getExtensionFn(hostPtr, extensionId);
    }
}
