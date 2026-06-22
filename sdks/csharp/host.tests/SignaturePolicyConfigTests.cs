using System;
using System.Runtime.InteropServices;
using Polyplug.Abi;
using Polyplug.Host;
using Xunit;

namespace Polyplug.Host.Tests
{
    /// <summary>
    /// Verifies the host SDK exposes the bundle signature policy: the
    /// <see cref="SignaturePolicy"/> enum maps to the ABI #[repr(u32)] values,
    /// the <c>RuntimeConfig.SignaturePolicy</c> field marshals at offset 44 (the
    /// struct stays 48 bytes), and the <c>RuntimeBuilder.SignaturePolicy</c>
    /// setter drives a runtime create without crashing across the FFI boundary.
    /// Enforcement DELIVERY on a real signed load is out of scope here.
    /// </summary>
    public sealed class SignaturePolicyConfigTests
    {
        static SignaturePolicyConfigTests()
        {
            TestNativeLibraries.EnsureInstalled();
        }

        [Fact]
        public void EnumValuesMatchAbi()
        {
            Assert.Equal(0u, (uint)SignaturePolicy.Off);
            Assert.Equal(1u, (uint)SignaturePolicy.WarnOnly);
            Assert.Equal(2u, (uint)SignaturePolicy.Required);
        }

        [Fact]
        public void RuntimeConfigIs48BytesWithPolicyAtOffset44()
        {
            Assert.Equal(48, Marshal.SizeOf<RuntimeConfig>());
            Assert.Equal(44, Marshal.OffsetOf<RuntimeConfig>(nameof(RuntimeConfig.SignaturePolicy)).ToInt32());
        }

        [Fact]
        public void RequiredMarshalsAsValue2AtOffset44()
        {
            RuntimeConfig config = new RuntimeConfig
            {
                Compatibility = Compatibility.Strict,
                SignaturePolicy = SignaturePolicy.Required,
            };

            nint ptr = Marshal.AllocHGlobal(Marshal.SizeOf<RuntimeConfig>());
            try
            {
                Marshal.StructureToPtr(config, ptr, fDeleteOld: false);
                uint raw = (uint)Marshal.ReadInt32(ptr, 44);
                Assert.Equal((uint)SignaturePolicy.Required, raw);
                Assert.Equal(2u, raw);
            }
            finally
            {
                Marshal.FreeHGlobal(ptr);
            }
        }

        [Fact]
        public void ZeroedConfigDefaultsToOff()
        {
            // A default-constructed RuntimeConfig must encode SignaturePolicy.Off (0),
            // preserving existing behavior when the setter is not used.
            RuntimeConfig config = default;
            Assert.Equal(SignaturePolicy.Off, config.SignaturePolicy);
        }

        [Fact]
        public void BuilderSignaturePolicyCreatesRuntime()
        {
            // The setter routes through BuildWithConfig, which marshals a
            // RuntimeConfig carrying the policy and calls polyplug_runtime_create.
            Runtime runtime = new RuntimeBuilder()
                .SignaturePolicy(SignaturePolicy.Required)
                .Build();
            Assert.NotEqual(nint.Zero, runtime.HostHandle);
            GC.KeepAlive(runtime);
        }
    }
}
