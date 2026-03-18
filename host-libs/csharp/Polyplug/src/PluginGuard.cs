using System;

namespace Polyplug;

public struct PluginGuard : IDisposable
{
    private sealed class GuardReleaser
    {
        private nint _handle;

        public GuardReleaser(nint handle)
        {
            _handle = handle;
        }

        ~GuardReleaser()
        {
            Release();
        }

        public void Release()
        {
            if (_handle != nint.Zero)
            {
                NativeMethods.PolyplugRuntimePluginRelease(_handle);
                _handle = nint.Zero;
            }
        }
    }

    private nint _guardHandle;
    private nint _vtablePtr;
    private GuardReleaser? _releaser;

    internal PluginGuard(nint guardHandle, nint vtablePtr)
    {
        _guardHandle = guardHandle;
        _vtablePtr = vtablePtr;
        _releaser = guardHandle == nint.Zero ? null : new GuardReleaser(guardHandle);
    }

    public readonly nint GetVTable()
    {
        if (_guardHandle == nint.Zero)
        {
            throw new ObjectDisposedException(nameof(PluginGuard));
        }

        return _vtablePtr;
    }

    public readonly bool IsNull()
    {
        return _guardHandle == nint.Zero;
    }

    public void Dispose()
    {
        _releaser?.Release();
        _releaser = null;
        _guardHandle = nint.Zero;
        _vtablePtr = nint.Zero;
    }
}