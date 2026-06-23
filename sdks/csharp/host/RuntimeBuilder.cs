using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

using Polyplug.Abi;

namespace Polyplug.Host;

/// <summary>
/// Fluent builder for <see cref="Runtime"/> instances.
///
/// Mirrors the Rust <c>RuntimeBuilder</c> semantics: plugin directories added
/// via <see cref="PluginDir"/> are scanned during <see cref="Build"/> and every
/// bundle found (a subdirectory containing <c>manifest.toml</c>) is loaded, in
/// sorted order. Note that bundles requiring a language loader can only load
/// after that loader is registered — hosts that register loaders after
/// <c>Build()</c> must load such bundles explicitly instead of using
/// <see cref="PluginDir"/>.
/// </summary>
public sealed class RuntimeBuilder
{
    private readonly List<string> _pluginDirs;
    private readonly List<byte[]> _trustedKeys;
    private Action<ReloadPhase>? _onReload;
    private SignaturePolicy? _signaturePolicy;

    public RuntimeBuilder()
    {
        _pluginDirs = [];
        _trustedKeys = [];
        _onReload = null;
        _signaturePolicy = null;
    }

    /// <summary>Add a directory to scan for plugin bundles during <see cref="Build"/>.</summary>
    public RuntimeBuilder PluginDir(string path)
    {
        if (path is null)
        {
            throw new ArgumentNullException(nameof(path));
        }

        _pluginDirs.Add(path);
        return this;
    }

    /// <summary>
    /// Register a callback invoked on hot-reload phase transitions.
    /// The callback is owned by the built <see cref="Runtime"/> instance
    /// (per-instance, no process-global state) and stays alive until the
    /// runtime is destroyed.
    /// </summary>
    public RuntimeBuilder OnReload(Action<ReloadPhase> callback)
    {
        if (callback is null)
        {
            throw new ArgumentNullException(nameof(callback));
        }

        _onReload = callback;
        return this;
    }

    /// <summary>
    /// Set the bundle signature enforcement policy
    /// (<see cref="SignaturePolicy"/> discriminant). Defaults to
    /// <see cref="SignaturePolicy.Off"/> (unsigned bundles load normally)
    /// when never set.
    /// </summary>
    public RuntimeBuilder SignaturePolicy(SignaturePolicy policy)
    {
        _signaturePolicy = policy;
        return this;
    }

    /// <summary>
    /// Pin the trusted Ed25519 verifying-key allowlist (key pinning). Each entry
    /// is a 32-byte verifying key. When non-empty AND the
    /// <see cref="SignaturePolicy"/> is not <see cref="SignaturePolicy.Off"/>,
    /// the runtime requires every bundle's embedded signing key to be a member of
    /// this allowlist (a re-signed bundle with an attacker key is rejected).
    /// Empty (the default) = Trust-On-First-Use: the embedded key is trusted
    /// without pinning.
    ///
    /// The keys are copied into the builder. The runtime copies the key bytes
    /// out of <c>RuntimeConfig.TrustedKeys</c> during
    /// <c>polyplug_runtime_create</c>; the unmanaged buffer is only needed for
    /// that call and is freed as soon as create returns.
    /// </summary>
    public RuntimeBuilder TrustedKeys(IReadOnlyList<byte[]> keys)
    {
        if (keys is null)
        {
            throw new ArgumentNullException(nameof(keys));
        }

        _trustedKeys.Clear();
        foreach (byte[] key in keys)
        {
            if (key is null)
            {
                throw new ArgumentNullException(nameof(keys), "trusted key entry must not be null.");
            }
            if (key.Length != Ed25519PublicKeyBytes)
            {
                throw new ArgumentException(
                    $"each trusted key must be exactly {Ed25519PublicKeyBytes} bytes (got {key.Length}).",
                    nameof(keys));
            }
            _trustedKeys.Add((byte[])key.Clone());
        }
        return this;
    }

    public Runtime Build()
    {
        bool hasKeys = _trustedKeys.Count > 0;
        Runtime runtime = _onReload is null && !hasKeys && _signaturePolicy is null
            ? BuildDefault()
            : _onReload is null
                ? BuildWithConfig(_signaturePolicy, _trustedKeys)
                : BuildWithReloadCallback(_onReload, _signaturePolicy, _trustedKeys);
        LoadPluginDirs(runtime);
        return runtime;
    }

    /// <summary>Size of an <c>Ed25519PublicKey</c> (32 raw bytes).</summary>
    internal const int Ed25519PublicKeyBytes = 32;

    /// <summary>
    /// Alignment of <c>Ed25519PublicKey</c>. The ABI mirror documents it as
    /// <c>#[repr(C)]</c>, 32 bytes, align 1 — a bare byte array with no padding.
    /// The value is reported through <c>RuntimeConfig.TrustedKeysAlign</c> so the
    /// runtime sees the same element alignment Rust uses (<c>align_of</c>).
    /// </summary>
    internal const int Ed25519PublicKeyAlign = 1;

    private static Runtime BuildDefault()
    {
        nint defaultHandle = Runtime.CreateNative();
        if (defaultHandle == nint.Zero)
        {
            Runtime.ThrowLastError("Failed to create runtime.");
        }

        return new Runtime(defaultHandle, default, default);
    }

    /// <summary>
    /// Marshal the trusted-key list into a freshly allocated unmanaged buffer and
    /// stamp <c>config.TrustedKeys/TrustedKeysLen/TrustedKeysAlign</c>. Returns the
    /// buffer pointer (caller owns it) or <c>nint.Zero</c> for an empty list. The
    /// runtime copies the key bytes during <c>polyplug_runtime_create</c>, so the
    /// caller frees this buffer as soon as create returns.
    /// </summary>
    internal static nint MarshalTrustedKeys(IReadOnlyList<byte[]> keys, ref RuntimeConfig config)
    {
        if (keys.Count == 0)
        {
            return nint.Zero;
        }

        nint buffer = Marshal.AllocHGlobal(Ed25519PublicKeyBytes * keys.Count);
        for (int i = 0; i < keys.Count; i++)
        {
            Marshal.Copy(keys[i], 0, buffer + i * Ed25519PublicKeyBytes, Ed25519PublicKeyBytes);
        }

        config.TrustedKeys = buffer;
        config.TrustedKeysLen = (nuint)keys.Count;
        config.TrustedKeysAlign = (nuint)Ed25519PublicKeyAlign;
        return buffer;
    }

    private static Runtime BuildWithConfig(SignaturePolicy? signaturePolicy, IReadOnlyList<byte[]> trustedKeys)
    {
        RuntimeConfig config = new RuntimeConfig
        {
            Compatibility = Compatibility.Strict,
            SignaturePolicy = signaturePolicy ?? Polyplug.Abi.SignaturePolicy.Off,
        };

        nint keysBuffer = MarshalTrustedKeys(trustedKeys, ref config);
        nint handle;
        try
        {
            nint configPtr = Marshal.AllocHGlobal(Marshal.SizeOf<RuntimeConfig>());
            try
            {
                Marshal.StructureToPtr(config, configPtr, fDeleteOld: false);
                handle = NativeMethods.PolyplugRuntimeCreate(configPtr);
            }
            finally
            {
                Marshal.FreeHGlobal(configPtr);
            }
        }
        finally
        {
            // The runtime copied the key bytes during create; free the buffer
            // now (also on the throw path).
            if (keysBuffer != nint.Zero)
            {
                Marshal.FreeHGlobal(keysBuffer);
            }
        }

        if (handle == nint.Zero)
        {
            Runtime.ThrowLastError("Failed to create runtime.");
        }

        return new Runtime(handle, default, default);
    }

    /// <summary>
    /// Create the runtime with a marshaled <see cref="RuntimeConfig"/> carrying
    /// the reload trampoline. The callback state GCHandle travels as
    /// <c>OnReloadUserData</c> (recovered per-invocation by the static
    /// trampoline); ownership of both GCHandles transfers to the Runtime.
    /// The native core copies the config during the call, so the unmanaged
    /// copy is freed once create returns.
    /// </summary>
    private static Runtime BuildWithReloadCallback(
        Action<ReloadPhase> callback,
        SignaturePolicy? signaturePolicy,
        IReadOnlyList<byte[]> trustedKeys)
    {
        Runtime.ReloadCallbackState state = new Runtime.ReloadCallbackState(callback);
        Runtime.OnReloadTrampoline trampoline = Runtime.OnReloadNative;
        GCHandle stateHandle = GCHandle.Alloc(state);
        GCHandle trampolineHandle = GCHandle.Alloc(trampoline);
        nint keysBuffer = nint.Zero;

        try
        {
            RuntimeConfig config = new RuntimeConfig
            {
                Compatibility = Compatibility.Strict,
                HotReloadEnabled = true,
                OnReload = Marshal.GetFunctionPointerForDelegate(trampoline),
                OnReloadUserData = GCHandle.ToIntPtr(stateHandle),
                SignaturePolicy = signaturePolicy ?? Polyplug.Abi.SignaturePolicy.Off,
            };

            keysBuffer = MarshalTrustedKeys(trustedKeys, ref config);

            nint handle;
            try
            {
                nint configPtr = Marshal.AllocHGlobal(Marshal.SizeOf<RuntimeConfig>());
                try
                {
                    Marshal.StructureToPtr(config, configPtr, fDeleteOld: false);
                    handle = NativeMethods.PolyplugRuntimeCreate(configPtr);
                }
                finally
                {
                    Marshal.FreeHGlobal(configPtr);
                }
            }
            finally
            {
                // The runtime copied the key bytes during create; free the
                // buffer now (also on the throw path).
                if (keysBuffer != nint.Zero)
                {
                    Marshal.FreeHGlobal(keysBuffer);
                    keysBuffer = nint.Zero;
                }
            }

            if (handle == nint.Zero)
            {
                Runtime.ThrowLastError("Failed to create runtime.");
            }

            return new Runtime(handle, stateHandle, trampolineHandle);
        }
        catch
        {
            // Creation failed before a Runtime took ownership of the GCHandles —
            // release here.
            stateHandle.Free();
            trampolineHandle.Free();
            throw;
        }
    }

    /// <summary>
    /// Scan each stored plugin directory for bundle subdirectories (containing
    /// <c>manifest.toml</c>) and load them in sorted order — mirroring the Rust
    /// builder's scan-and-load-at-build semantics.
    /// </summary>
    private void LoadPluginDirs(Runtime runtime)
    {
        foreach (string dir in _pluginDirs)
        {
            if (!Directory.Exists(dir))
            {
                continue;
            }

            string[] bundleDirs = Directory.GetDirectories(dir);
            // System.Array spelled out: `using Polyplug.Abi` brings the ABI `Array` struct into scope.
            System.Array.Sort(bundleDirs, StringComparer.Ordinal);
            foreach (string bundleDir in bundleDirs)
            {
                if (File.Exists(Path.Combine(bundleDir, "manifest.toml")))
                {
                    runtime.LoadBundle(bundleDir);
                }
            }
        }
    }
}
