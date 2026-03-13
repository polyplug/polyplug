using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug;

public sealed class Runtime {
    internal const string NativeLib = "polyplug";
    private IntPtr _handle;

    private Runtime(IntPtr handle) {
        _handle = handle;
    }

    public static RuntimeBuilder Builder() => new RuntimeBuilder();

    internal static Runtime Create(IntPtr handle) => new Runtime(handle);

    ~Runtime() {
        if (_handle != IntPtr.Zero) {
            polyplug_runtime_free(_handle);
            _handle = IntPtr.Zero;
        }
    }

    public void LoadBundle(string path) {
        EnsureHandle();
        InvokeWithUtf8(path, (ptr, len) => {
            uint result = polyplug_load_bundle(_handle, ptr, len);
            if (result != 0u) {
                ThrowLastError("Failed to load bundle.");
            }
        });
    }

    public void ReloadBundle(string path) {
        EnsureHandle();
        InvokeWithUtf8(path, (ptr, len) => {
            uint result = polyplug_reload_bundle(_handle, ptr, len);
            if (result != 0u) {
                ThrowLastError("Failed to reload bundle.");
            }
        });
    }

    public ulong FindByContract(ulong contractId, uint minVersion) {
        EnsureHandle();
        ulong packed = polyplug_rt_find_by_contract(_handle, contractId, minVersion);
        return packed;
    }

    public ulong FindByBundle(ulong bundleId, ulong contractId, uint minVersion) {
        EnsureHandle();
        ulong packed = polyplug_rt_find_by_bundle(_handle, bundleId, contractId, minVersion);
        return packed;
    }

    public ulong[] FindAllByContract(ulong contractId, uint minVersion) {
        EnsureHandle();
        int capacity = 16;
        while (true) {
            ulong[] handles = new ulong[capacity];
            GCHandle pinned = GCHandle.Alloc(handles, GCHandleType.Pinned);
            try {
                IntPtr outPtr = pinned.AddrOfPinnedObject();
                UIntPtr outCap = (UIntPtr)handles.Length;
                UIntPtr written = polyplug_rt_find_all_by_contract(
                    _handle,
                    contractId,
                    minVersion,
                    outPtr,
                    outCap
                );
                ulong count = written.ToUInt64();
                if (count == 0ul) {
                    return Array.Empty<ulong>();
                }
                if (count < (ulong)handles.Length) {
                    ulong[] result = new ulong[count];
                    Array.Copy(handles, result, (long)count);
                    return result;
                }
            } finally {
                pinned.Free();
            }
            capacity = checked(capacity * 2);
        }
    }

    public PluginGuard ResolvePlugin(ulong packedHandle) {
        EnsureHandle();
        if (packedHandle == ulong.MaxValue) {
            return new PluginGuard(IntPtr.Zero);
        }
        IntPtr guard = polyplug_rt_resolve_plugin(_handle, packedHandle);
        if (guard == IntPtr.Zero) {
            ThrowLastError("Failed to resolve plugin.");
        }
        return new PluginGuard(guard);
    }

    public IntPtr GetExtension(uint extensionId) {
        _ = extensionId;
        return IntPtr.Zero;
    }

    private void EnsureHandle() {
        if (_handle == IntPtr.Zero) {
            throw new ObjectDisposedException(nameof(Runtime));
        }
    }

    private static void InvokeWithUtf8(string value, Action<IntPtr, UIntPtr> action) {
        if (value == null) {
            throw new ArgumentNullException(nameof(value));
        }
        byte[] bytes = Encoding.UTF8.GetBytes(value);
        int length = bytes.Length;
        int allocSize = length == 0 ? 1 : length;
        IntPtr ptr = Marshal.AllocHGlobal(allocSize);
        try {
            if (length > 0) {
                Marshal.Copy(bytes, 0, ptr, length);
            }
            action(ptr, (UIntPtr)length);
        } finally {
            Marshal.FreeHGlobal(ptr);
        }
    }

    internal static void ThrowLastError(string fallbackMessage) {
        string message = GetLastError();
        if (string.IsNullOrEmpty(message)) {
            message = fallbackMessage;
        }
        throw new InvalidOperationException(message);
    }

    private static string GetLastError() {
        UIntPtr len = polyplug_error_message_len();
        ulong length = len.ToUInt64();
        if (length == 0ul) {
            return string.Empty;
        }
        if (length > int.MaxValue) {
            return "polyplug error message too large";
        }
        byte[] buffer = new byte[(int)length];
        GCHandle pinned = GCHandle.Alloc(buffer, GCHandleType.Pinned);
        try {
            UIntPtr written = polyplug_last_error(pinned.AddrOfPinnedObject(), (UIntPtr)buffer.Length);
            int count = (int)written.ToUInt64();
            if (count == 0) {
                return string.Empty;
            }
            return Encoding.UTF8.GetString(buffer, 0, count);
        } finally {
            pinned.Free();
        }
    }

    internal static IntPtr GetVTablePtr(IntPtr guard) => polyplug_get_vtable(guard);

    internal static void ReleaseGuard(IntPtr guard) {
        if (guard != IntPtr.Zero) {
            polyplug_guard_free(guard);
        }
    }

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_new", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_runtime_new();

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_free", CallingConvention = CallingConvention.Cdecl)]
    private static extern void polyplug_runtime_free(IntPtr rt);

    [DllImport(NativeLib, EntryPoint = "polyplug_load_bundle", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint polyplug_load_bundle(IntPtr rt, IntPtr path, UIntPtr pathLen);

    [DllImport(NativeLib, EntryPoint = "polyplug_reload_bundle", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint polyplug_reload_bundle(IntPtr rt, IntPtr path, UIntPtr pathLen);

    [DllImport(NativeLib, EntryPoint = "polyplug_rt_find_by_contract", CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong polyplug_rt_find_by_contract(IntPtr rt, ulong contractId, uint minVersion);

    [DllImport(NativeLib, EntryPoint = "polyplug_rt_find_by_bundle", CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong polyplug_rt_find_by_bundle(IntPtr rt, ulong bundleId, ulong contractId, uint minVersion);

    [DllImport(NativeLib, EntryPoint = "polyplug_rt_find_all_by_contract", CallingConvention = CallingConvention.Cdecl)]
    private static extern UIntPtr polyplug_rt_find_all_by_contract(
        IntPtr rt,
        ulong contractId,
        uint minVersion,
        IntPtr outHandles,
        UIntPtr outCap
    );

    [DllImport(NativeLib, EntryPoint = "polyplug_rt_resolve_plugin", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_rt_resolve_plugin(IntPtr rt, ulong packedHandle);

    [DllImport(NativeLib, EntryPoint = "polyplug_get_vtable", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_get_vtable(IntPtr guard);

    [DllImport(NativeLib, EntryPoint = "polyplug_guard_free", CallingConvention = CallingConvention.Cdecl)]
    private static extern void polyplug_guard_free(IntPtr guard);

    [DllImport(NativeLib, EntryPoint = "polyplug_last_error", CallingConvention = CallingConvention.Cdecl)]
    private static extern UIntPtr polyplug_last_error(IntPtr buf, UIntPtr bufLen);

    [DllImport(NativeLib, EntryPoint = "polyplug_error_message_len", CallingConvention = CallingConvention.Cdecl)]
    private static extern UIntPtr polyplug_error_message_len();

}

public struct PluginGuard : IDisposable {
    private IntPtr _handle;
    private GuardReleaser? _releaser;

    internal PluginGuard(IntPtr handle) {
        _handle = handle;
        _releaser = new GuardReleaser(handle);
    }

    public IntPtr GetVTable() {
        if (_handle == IntPtr.Zero) {
            return IntPtr.Zero;
        }
        return Runtime.GetVTablePtr(_handle);
    }

    public void Dispose() {
        _releaser?.Release();
        _releaser = null;
        _handle = IntPtr.Zero;
    }

    private sealed class GuardReleaser {
        private IntPtr _handle;

        public GuardReleaser(IntPtr handle) {
            _handle = handle;
        }

        ~GuardReleaser() {
            Release();
        }

        public void Release() {
            if (_handle != IntPtr.Zero) {
                Runtime.ReleaseGuard(_handle);
                _handle = IntPtr.Zero;
            }
        }
    }
}

public sealed class RuntimeBuilder {
    private readonly List<string> _pluginDirs = new List<string>();
    private uint _compatibilityMode;

    public RuntimeBuilder PluginDir(string path) {
        if (path == null) {
            throw new ArgumentNullException(nameof(path));
        }
        _pluginDirs.Add(path);
        return this;
    }

    public RuntimeBuilder Compatibility(uint mode) {
        _compatibilityMode = mode;
        return this;
    }

    public Runtime Init() {
        _ = _compatibilityMode;
        _ = _pluginDirs;
        IntPtr handle = polyplug_runtime_new();
        if (handle == IntPtr.Zero) {
            Runtime.ThrowLastError("Failed to create runtime.");
        }
        return Runtime.Create(handle);
    }

    [DllImport(Runtime.NativeLib, EntryPoint = "polyplug_runtime_new", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_runtime_new();
}
