using System;
using System.Runtime.CompilerServices;

namespace Polyplug.Host;

/// <summary>
/// Guard for a resolved plugin handle.
/// Stores runtime + handle for hot-reload safety.
/// Re-resolves vtable on each call to detect stale handles.
/// </summary>
public readonly struct PluginGuard
{
    private readonly nint _rt;
    private readonly ulong _handle;

    internal PluginGuard(nint rt, ulong handle)
    {
        _rt = rt;
        _handle = handle;
    }

    /// <summary>
    /// Re-resolves vtable on each call (hot-reload safe).
    /// Returns nint.Zero if this is a null guard or resolution fails.
    /// </summary>
    public readonly nint GetVTable()
    {
        if (_rt == nint.Zero || _handle == ulong.MaxValue)
        {
            return nint.Zero;
        }

        return NativeMethods.PolyplugRuntimeResolvePlugin(_rt, _handle);
    }

    /// <summary>
    /// Returns the stored handle.
    /// </summary>
    public readonly ulong GetHandle()
    {
        return _handle;
    }

    /// <summary>
    /// Returns true if this guard is null (no runtime or null handle).
    /// </summary>
    public readonly bool IsNull()
    {
        return _rt == nint.Zero || _handle == ulong.MaxValue;
    }

    /// <summary>
    /// Returns a null guard (no runtime, null handle).
    /// </summary>
    public static PluginGuard Reset()
    {
        return new PluginGuard(nint.Zero, ulong.MaxValue);
    }
}