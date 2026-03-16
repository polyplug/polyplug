using System;

namespace Polyplug;

public struct PluginGuard : IDisposable
{
    private nint _handle;
    private GuardReleaser? _releaser;

    internal PluginGuard(nint handle)
    {
        _handle = handle;
        _releaser = new GuardReleaser(handle);
    }

    public nint GetVTable()
    {
        if (_handle == nint.Zero)
        {
            return nint.Zero;
        }

        return Runtime.GetVTablePtr(_handle);
    }

    public void Dispose()
    {
        _releaser?.Release();
        _releaser = null;
        _handle = nint.Zero;
    }

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
                Runtime.ReleaseGuard(_handle);
                _handle = nint.Zero;
            }
        }
    }
}
