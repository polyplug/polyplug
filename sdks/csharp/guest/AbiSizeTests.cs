// AbiSizeTests.cs — ABI struct layout regression tests.
// These assertions fire at static construction time in Debug builds.
// Marshal.SizeOf<T> verifies the blittable layout matches the Rust #[repr(C)] counterparts.
using System.Diagnostics;
using System.Runtime.InteropServices;
using Polyplug.Abi;

namespace Polyplug.Guest;

/// <summary>
/// ABI struct size assertions. Every value must match the Rust #[repr(C)] counterpart exactly.
/// Mismatches indicate a broken ABI contract and will cause silent memory corruption.
/// </summary>
internal static class AbiSizeAssertions
{
    static AbiSizeAssertions()
    {
        // StringView: IntPtr(8) + nuint(8) = 16 bytes
        Debug.Assert(Marshal.SizeOf<StringView>() == 16,
            $"StringView size mismatch: {Marshal.SizeOf<StringView>()} != 16");

        // Buffer: IntPtr(8) + nuint(8) + nuint(8) = 24 bytes
        Debug.Assert(Marshal.SizeOf<Polyplug.Abi.Buffer>() == 24,
            $"Buffer size mismatch: {Marshal.SizeOf<Polyplug.Abi.Buffer>()} != 24");

        // AbiError: uint(4) + _pad(4) + StringView(16) = 24 bytes
        Debug.Assert(Marshal.SizeOf<AbiError>() == 24,
            $"AbiError size mismatch: {Marshal.SizeOf<AbiError>()} != 24");

        // GuestContractHandle: uint(4) + uint(4) = 8 bytes
        Debug.Assert(Marshal.SizeOf<GuestContractHandle>() == 8,
            $"GuestContractHandle size mismatch: {Marshal.SizeOf<GuestContractHandle>()} != 8");

        // HostContext: IntPtr(8) + ulong(8) = 16 bytes
        Debug.Assert(Marshal.SizeOf<HostContext>() == 16,
            $"HostContext size mismatch: {Marshal.SizeOf<HostContext>()} != 16");

        // NativeDispatch: IntPtr(8) = 8 bytes
        Debug.Assert(Marshal.SizeOf<NativeDispatch>() == 8,
            $"NativeDispatch size mismatch: {Marshal.SizeOf<NativeDispatch>()} != 8");

        // VmDispatch: IntPtr(8) + IntPtr(8) = 16 bytes
        Debug.Assert(Marshal.SizeOf<VmDispatch>() == 16,
            $"VmDispatch size mismatch: {Marshal.SizeOf<VmDispatch>()} != 16");

        // RuntimeAbi: 8 x IntPtr(8) = 64 bytes
        Debug.Assert(Marshal.SizeOf<RuntimeAbi>() == 64,
            $"RuntimeAbi size mismatch: {Marshal.SizeOf<RuntimeAbi>()} != 64");

        // PluginDescriptor: StringView(16) + StringView(16) + uint(4)*3 + _pad(4) = 48 bytes
        Debug.Assert(Marshal.SizeOf<PluginDescriptor>() == 48,
            $"PluginDescriptor size mismatch: {Marshal.SizeOf<PluginDescriptor>()} != 48");

        // PluginContext: StringView(16) + uint(4) + _pad(4) + ulong(8) = 32 bytes
        Debug.Assert(Marshal.SizeOf<PluginContext>() == 32,
            $"PluginContext size mismatch: {Marshal.SizeOf<PluginContext>()} != 32");

        // ExtensionEntry: uint(4) + _pad(4) + IntPtr(8) = 16 bytes
        Debug.Assert(Marshal.SizeOf<ExtensionEntry>() == 16,
            $"ExtensionEntry size mismatch: {Marshal.SizeOf<ExtensionEntry>()} != 16");
    }

    // Touch this class to trigger assertions in test/debug builds.
    internal static void Verify() { }  // static ctor runs on first access
}