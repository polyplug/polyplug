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
        public void RuntimeConfigIs72BytesWithPolicyAtOffset44AndTrustedKeysAtOffset48()
        {
            Assert.Equal(72, Marshal.SizeOf<RuntimeConfig>());
            Assert.Equal(44, Marshal.OffsetOf<RuntimeConfig>(nameof(RuntimeConfig.SignaturePolicy)).ToInt32());
            Assert.Equal(48, Marshal.OffsetOf<RuntimeConfig>(nameof(RuntimeConfig.TrustedKeys)).ToInt32());
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

        [Fact]
        public void TrustedKeysMarshalIntoConfig()
        {
            // Drive the EXACT production marshalling helper RuntimeBuilder uses and
            // assert the resulting RuntimeConfig points at a contiguous Ed25519
            // buffer with the right element count and alignment.
            byte[] k1 = new byte[32];
            byte[] k2 = new byte[32];
            for (int i = 0; i < 32; i++)
            {
                k1[i] = (byte)i;
                k2[i] = (byte)(0x80 + i);
            }

            RuntimeConfig config = new RuntimeConfig { Compatibility = Compatibility.Strict };
            nint buffer = RuntimeBuilder.MarshalTrustedKeys([k1, k2], ref config);
            try
            {
                Assert.NotEqual(nint.Zero, config.TrustedKeys);
                Assert.Equal(buffer, config.TrustedKeys);
                Assert.Equal((nuint)2, config.TrustedKeysLen);
                Assert.Equal((nuint)RuntimeBuilder.Ed25519PublicKeyAlign, config.TrustedKeysAlign);
                Assert.Equal((nuint)1, config.TrustedKeysAlign);

                // The bytes landed contiguously: element 0 = k1, element 1 = k2.
                byte[] readBack = new byte[64];
                Marshal.Copy(buffer, readBack, 0, 64);
                for (int i = 0; i < 32; i++)
                {
                    Assert.Equal(k1[i], readBack[i]);
                    Assert.Equal(k2[i], readBack[32 + i]);
                }
            }
            finally
            {
                if (buffer != nint.Zero)
                {
                    Marshal.FreeHGlobal(buffer);
                }
            }
        }

        [Fact]
        public void EmptyTrustedKeysLeaveConfigAtDefault()
        {
            RuntimeConfig config = new RuntimeConfig { Compatibility = Compatibility.Strict };
            nint buffer = RuntimeBuilder.MarshalTrustedKeys([], ref config);
            Assert.Equal(nint.Zero, buffer);
            Assert.Equal(nint.Zero, config.TrustedKeys);
            Assert.Equal((nuint)0, config.TrustedKeysLen);
            Assert.Equal((nuint)0, config.TrustedKeysAlign);
        }

        [Fact]
        public void TrustedKeysRejectsWrongLength()
        {
            Assert.Throws<ArgumentException>(() =>
                new RuntimeBuilder().TrustedKeys([new byte[31]]));
            Assert.Throws<ArgumentException>(() =>
                new RuntimeBuilder().TrustedKeys([new byte[33]]));
        }

        [Fact]
        public void TrustedKeysRejectsNull()
        {
            Assert.Throws<ArgumentNullException>(() =>
                new RuntimeBuilder().TrustedKeys(null!));
        }

        [Fact]
        public void BuilderTrustedKeysCreatesRuntime()
        {
            // Pinning keys must route through the config-carrying build branch and
            // create a real runtime; the runtime copies the keys during create and
            // the transient buffer is freed once Build() returns.
            Runtime runtime = new RuntimeBuilder()
                .SignaturePolicy(SignaturePolicy.WarnOnly)
                .TrustedKeys([new byte[32], new byte[32]])
                .Build();
            Assert.NotEqual(nint.Zero, runtime.HostHandle);
            GC.KeepAlive(runtime);
        }
    }
}
