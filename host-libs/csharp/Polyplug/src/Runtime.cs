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

    public IntPtr Handle => _handle;

    public static RuntimeBuilder Builder() => new RuntimeBuilder();

    internal static Runtime Create(IntPtr handle) => new Runtime(handle);

    ~Runtime() {
        if (_handle != IntPtr.Zero) {
            polyplug_runtime_destroy(_handle);
            _handle = IntPtr.Zero;
        }
    }

    public void LoadBundle(string path) {
        EnsureHandle();
        InvokeWithUtf8(path, (ptr, len) => {
            uint result = polyplug_runtime_load_bundle(_handle, ptr, len);
            if (result != 0u) {
                ThrowLastError("Failed to load bundle.");
            }
        });
    }

    public void ReloadBundle(string path) {
        EnsureHandle();
        InvokeWithUtf8(path, (ptr, len) => {
            uint result = polyplug_runtime_reload_bundle(_handle, ptr, len);
            if (result != 0u) {
                ThrowLastError("Failed to reload bundle.");
            }
        });
    }

    public ulong FindByContract(ulong contractId, uint minVersion) {
        EnsureHandle();
        ulong packed = polyplug_runtime_find_by_contract(_handle, contractId, minVersion);
        return packed;
    }

    public ulong FindByBundle(ulong bundleId, ulong contractId, uint minVersion) {
        EnsureHandle();
        ulong packed = polyplug_runtime_find_by_bundle(_handle, bundleId, contractId, minVersion);
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
                UIntPtr written = polyplug_runtime_find_all_by_contract(
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
        IntPtr guard = polyplug_runtime_resolve_plugin(_handle, packedHandle);
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
            UIntPtr len = polyplug_runtime_error_message_len();
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
            UIntPtr written = polyplug_runtime_last_error(pinned.AddrOfPinnedObject(), (UIntPtr)buffer.Length);
            int count = (int)written.ToUInt64();
            if (count == 0) {
                return string.Empty;
            }
            return Encoding.UTF8.GetString(buffer, 0, count);
        } finally {
            pinned.Free();
        }
    }

    internal static IntPtr GetVTablePtr(IntPtr guard) => polyplug_runtime_plugin_vtable(guard);

    internal static void ReleaseGuard(IntPtr guard) {
        if (guard != IntPtr.Zero) {
            polyplug_runtime_plugin_release(guard);
        }
    }

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_runtime_create();

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_destroy", CallingConvention = CallingConvention.Cdecl)]
    private static extern void polyplug_runtime_destroy(IntPtr rt);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_load_bundle", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint polyplug_runtime_load_bundle(IntPtr rt, IntPtr path, UIntPtr pathLen);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_reload_bundle", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint polyplug_runtime_reload_bundle(IntPtr rt, IntPtr path, UIntPtr pathLen);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_find_by_contract", CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong polyplug_runtime_find_by_contract(IntPtr rt, ulong contractId, uint minVersion);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_find_by_bundle", CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong polyplug_runtime_find_by_bundle(IntPtr rt, ulong bundleId, ulong contractId, uint minVersion);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_find_all_by_contract", CallingConvention = CallingConvention.Cdecl)]
    private static extern UIntPtr polyplug_runtime_find_all_by_contract(
        IntPtr rt,
        ulong contractId,
        uint minVersion,
        IntPtr outHandles,
        UIntPtr outCap
    );

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_resolve_plugin", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_runtime_resolve_plugin(IntPtr rt, ulong packedHandle);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_plugin_vtable", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_runtime_plugin_vtable(IntPtr guard);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_plugin_release", CallingConvention = CallingConvention.Cdecl)]
    private static extern void polyplug_runtime_plugin_release(IntPtr guard);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_last_error", CallingConvention = CallingConvention.Cdecl)]
    private static extern UIntPtr polyplug_runtime_last_error(IntPtr buf, UIntPtr bufLen);

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_error_message_len", CallingConvention = CallingConvention.Cdecl)]
    private static extern UIntPtr polyplug_runtime_error_message_len();

    [DllImport(NativeLib, EntryPoint = "polyplug_runtime_register_loader", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint polyplug_runtime_register_loader(IntPtr rt, IntPtr loaderPtr);

    [DllImport("polyplug_dotnet", EntryPoint = "polyplug_dotnet_loader_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_dotnet_loader_create(IntPtr cfgPtr);

    [DllImport("polyplug_python", EntryPoint = "polyplug_python_loader_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_python_loader_create(IntPtr cfgPtr);

    [DllImport("polyplug_lua", EntryPoint = "polyplug_lua_loader_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_lua_loader_create(IntPtr cfgPtr);

    [DllImport("polyplug_js", EntryPoint = "polyplug_js_loader_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_js_loader_create(IntPtr cfgPtr);

    [DllImport("polyplug_native", EntryPoint = "polyplug_native_loader_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_native_loader_create(IntPtr cfgPtr);

    [StructLayout(LayoutKind.Sequential)]
    private struct DotnetLoaderConfig {
        public IntPtr MinFrameworkPtr;
        public UIntPtr MinFrameworkLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PythonLoaderConfig {
        public IntPtr MinVersionPtr;
        public UIntPtr MinVersionLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct EmptyLoaderConfig {
        public byte Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeLoaderConfig {
        public byte Reserved;
    }

    public void RegisterDotnetLoader(string minFramework = "10.0") {
        EnsureHandle();
        byte[] bytes = Encoding.UTF8.GetBytes(minFramework);
        GCHandle stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try {
            DotnetLoaderConfig cfg = new DotnetLoaderConfig {
                MinFrameworkPtr = stringHandle.AddrOfPinnedObject(),
                MinFrameworkLen = (UIntPtr)bytes.Length,
            };
            IntPtr cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<DotnetLoaderConfig>());
            try {
                Marshal.StructureToPtr(cfg, cfgPtr, false);
                IntPtr loaderPtr = polyplug_dotnet_loader_create(cfgPtr);
                if (loaderPtr == IntPtr.Zero) {
                    throw new InvalidOperationException("polyplug: dotnet loader create failed");
                }
                uint err = polyplug_runtime_register_loader(_handle, loaderPtr);
                if (err != 0u) {
                    ThrowLastError($"polyplug: dotnet loader register failed: {err}");
                }
            } finally {
                Marshal.FreeHGlobal(cfgPtr);
            }
        } finally {
            stringHandle.Free();
        }
    }

    public void RegisterPythonLoader(string minVersion = "3.11") {
        EnsureHandle();
        byte[] bytes = Encoding.UTF8.GetBytes(minVersion);
        GCHandle stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try {
            PythonLoaderConfig cfg = new PythonLoaderConfig {
                MinVersionPtr = stringHandle.AddrOfPinnedObject(),
                MinVersionLen = (UIntPtr)bytes.Length,
            };
            IntPtr cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<PythonLoaderConfig>());
            try {
                Marshal.StructureToPtr(cfg, cfgPtr, false);
                IntPtr loaderPtr = polyplug_python_loader_create(cfgPtr);
                if (loaderPtr == IntPtr.Zero) {
                    throw new InvalidOperationException("polyplug: python loader create failed");
                }
                uint err = polyplug_runtime_register_loader(_handle, loaderPtr);
                if (err != 0u) {
                    ThrowLastError($"polyplug: python loader register failed: {err}");
                }
            } finally {
                Marshal.FreeHGlobal(cfgPtr);
            }
        } finally {
            stringHandle.Free();
        }
    }

    public void RegisterLuaLoader() {
        EnsureHandle();
        EmptyLoaderConfig cfg = new EmptyLoaderConfig { Reserved = 0 };
        IntPtr cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<EmptyLoaderConfig>());
        try {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            IntPtr loaderPtr = polyplug_lua_loader_create(cfgPtr);
            if (loaderPtr == IntPtr.Zero) {
                throw new InvalidOperationException("polyplug: lua loader create failed");
            }
            uint err = polyplug_runtime_register_loader(_handle, loaderPtr);
            if (err != 0u) {
                ThrowLastError($"polyplug: lua loader register failed: {err}");
            }
        } finally {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }

    public void RegisterJsLoader() {
        EnsureHandle();
        EmptyLoaderConfig cfg = new EmptyLoaderConfig { Reserved = 0 };
        IntPtr cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<EmptyLoaderConfig>());
        try {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            IntPtr loaderPtr = polyplug_js_loader_create(cfgPtr);
            if (loaderPtr == IntPtr.Zero) {
                throw new InvalidOperationException("polyplug: js loader create failed");
            }
            uint err = polyplug_runtime_register_loader(_handle, loaderPtr);
            if (err != 0u) {
                ThrowLastError($"polyplug: js loader register failed: {err}");
            }
        } finally {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }

    public void RegisterNativeLoader() {
        EnsureHandle();
        NativeLoaderConfig cfg = new NativeLoaderConfig { Reserved = 0 };
        IntPtr cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<NativeLoaderConfig>());
        try {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            IntPtr loaderPtr = polyplug_native_loader_create(cfgPtr);
            if (loaderPtr == IntPtr.Zero) {
                throw new InvalidOperationException("polyplug: native loader create failed");
            }
            uint err = polyplug_runtime_register_loader(_handle, loaderPtr);
            if (err != 0u) {
                ThrowLastError($"polyplug: native loader register failed: {err}");
            }
        } finally {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }

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
        IntPtr handle = polyplug_runtime_create();
        if (handle == IntPtr.Zero) {
            Runtime.ThrowLastError("Failed to create runtime.");
        }
        return Runtime.Create(handle);
    }

    [DllImport(Runtime.NativeLib, EntryPoint = "polyplug_runtime_create", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr polyplug_runtime_create();
}
