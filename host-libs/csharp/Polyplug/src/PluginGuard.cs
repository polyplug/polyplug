using System;

namespace Polyplug;

public readonly struct PluginGuard
{
    private readonly nint _vtablePtr;

    internal PluginGuard(nint vtablePtr)
    {
        _vtablePtr = vtablePtr;
    }

    public readonly nint GetVTable()
    {
        if (_vtablePtr == nint.Zero)
        {
            throw new ObjectDisposedException(nameof(PluginGuard));
        }

        return _vtablePtr;
    }

    public readonly bool IsNull()
    {
        return _vtablePtr == nint.Zero;
    }
}