using System;
using System.Runtime.CompilerServices;

namespace Polyplug.Host;

/// <summary>
/// Guard for a resolved plugin handle.
/// Holds a ref-counted ResolveHandle that keeps the vtable alive.
/// Must call Release() or let the finalizer release the handle.
/// </summary>
public sealed class PluginGuard : IDisposable
{
    private nint _resolveHandle;

    internal PluginGuard(nint resolveHandle)
    {
        _resolveHandle = resolveHandle;
    }

    /// <summary>
    /// Get the vtable pointer from the ResolveHandle.
    /// Returns nint.Zero if this guard is null or has been released.
    /// </summary>
    public nint GetVTable()
    {
        if (_resolveHandle == nint.Zero)
        {
            return nint.Zero;
        }

        // ResolveHandle's first field is the vtable pointer (PluginInterface*)
        unsafe
        {
            return *(nint*)_resolveHandle;
        }
    }

    /// <summary>
    /// Returns true if this guard is null (no resolve handle).
    /// </summary>
    public bool IsNull()
    {
        return _resolveHandle == nint.Zero;
    }

    /// <summary>
    /// Release the resolve handle.
    /// </summary>
    public void Release()
    {
        if (_resolveHandle != nint.Zero)
        {
            NativeMethods.PolyplugRuntimeReleasePlugin(_resolveHandle);
            _resolveHandle = nint.Zero;
        }
    }

    /// <summary>
    /// Returns a null guard (no resolve handle).
    /// </summary>
    public static PluginGuard Reset()
    {
        return new PluginGuard(nint.Zero);
    }

    ~PluginGuard()
    {
        Release();
    }

    public void Dispose()
    {
        Release();
        GC.SuppressFinalize(this);
    }
}