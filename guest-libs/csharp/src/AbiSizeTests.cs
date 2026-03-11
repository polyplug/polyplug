// AbiSizeTests.cs — ABI struct layout regression tests.
// These assertions fire at static construction time in Debug builds.
// Marshal.SizeOf<T> verifies the blittable layout matches the Rust #[repr(C)] counterparts.
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace Polyplug.Guest;

/// <summary>
/// ABI struct size assertions. Every value must match the Rust #[repr(C)] counterpart exactly.
/// Mismatches indicate a broken ABI contract and will cause silent memory corruption.
/// </summary>
internal static class AbiSizeAssertions
{
    static AbiSizeAssertions()
    {
        // StringView: IntPtr(8) + ulong(8) = 16 bytes
        Debug.Assert(Marshal.SizeOf<StringView>() == 16,
            $"StringView size mismatch: {Marshal.SizeOf<StringView>()} != 16");

        // Buffer: IntPtr(8) + ulong(8) + ulong(8) = 24 bytes
        Debug.Assert(Marshal.SizeOf<Buffer>() == 24,
            $"Buffer size mismatch: {Marshal.SizeOf<Buffer>()} != 24");

        // AbiError: uint(4) + _pad(4) + StringView(16) = 24 bytes
        Debug.Assert(Marshal.SizeOf<AbiError>() == 24,
            $"AbiError size mismatch: {Marshal.SizeOf<AbiError>()} != 24");

        // PluginHandle: uint(4) + uint(4) = 8 bytes
        Debug.Assert(Marshal.SizeOf<PluginHandle>() == 8,
            $"PluginHandle size mismatch: {Marshal.SizeOf<PluginHandle>()} != 8");

        // PluginVTable: ulong(8) + uint(4) + uint(4) + IntPtr(8) = 24 bytes
        Debug.Assert(Marshal.SizeOf<PluginVTable>() == 24,
            $"PluginVTable size mismatch: {Marshal.SizeOf<PluginVTable>()} != 24");

        // HostVTable: 7 x IntPtr(8) = 56 bytes
        Debug.Assert(Marshal.SizeOf<HostVTable>() == 56,
            $"HostVTable size mismatch: {Marshal.SizeOf<HostVTable>()} != 56");

        // PluginDescriptor: StringView(16) + StringView(16) + uint(4)*3 + _pad(4) = 48 bytes
        Debug.Assert(Marshal.SizeOf<PluginDescriptor>() == 48,
            $"PluginDescriptor size mismatch: {Marshal.SizeOf<PluginDescriptor>()} != 48");

        // PluginRegistrar: IntPtr(8) + IntPtr(8) = 16 bytes
        Debug.Assert(Marshal.SizeOf<PluginRegistrar>() == 16,
            $"PluginRegistrar size mismatch: {Marshal.SizeOf<PluginRegistrar>()} != 16");
    }

    // Touch this class to trigger assertions in test/debug builds.
    internal static void Verify() { }  // static ctor runs on first access
}
