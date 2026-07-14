using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug.Abi;
using Polyplug.Host;
using Xunit;

namespace Polyplug.Host.Tests;

public sealed class RuntimeIntrospectionTests
{
    static RuntimeIntrospectionTests()
    {
        TestNativeLibraries.EnsureInstalled();
    }

    [Fact]
    public void SnapshotDescriptorsCopyEverySourceKindAndAllContractsAndFreeArrays()
    {
        using IntrospectionRuntimeFixture fixture = new(IntrospectionMode.Populated);

        IReadOnlyList<BundleDescriptor> bundles = fixture.Runtime.GetBundleDescriptors();
        Assert.Collection(
            bundles,
            bundle => AssertBundle(bundle, 10, "internal", BundleSourceKind.Internal),
            bundle => AssertBundle(bundle, 20, "path", BundleSourceKind.Path),
            bundle => AssertBundle(bundle, 30, "code", BundleSourceKind.Code),
            bundle => AssertBundle(bundle, 40, "bytes", BundleSourceKind.Bytes));

        IReadOnlyList<RegisteredContractDescriptor> contracts = fixture.Runtime.GetRegisteredContractDescriptors();
        Assert.Collection(
            contracts,
            contract => AssertContract(contract, 1, 11, 101, "alpha", "example.alpha"),
            contract => AssertContract(contract, 2, 12, 102, "beta", "example.beta"));

        Assert.Equal(10, fixture.PoisonedNativeBufferCount);

        Assert.Collection(
            fixture.FreeCalls,
            call => AssertFree(call, Encoding.UTF8.GetByteCount("internal"), 1),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("path"), 1),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("code"), 1),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("bytes"), 1),
            call => AssertFree(call, 4 * sizeof(ulong), 8),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("alpha"), 1),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("example.alpha"), 1),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("beta"), 1),
            call => AssertFree(call, Encoding.UTF8.GetByteCount("example.beta"), 1),
            call => AssertFree(call, 2 * Marshal.SizeOf<GuestContractHandle>(), 4));
    }

    [Fact]
    public void CurrentRuntimeWithEmptyResultsReturnsEmptySnapshots()
    {
        using IntrospectionRuntimeFixture fixture = new(IntrospectionMode.Empty);

        Assert.Empty(fixture.Runtime.GetBundleDescriptors());
        Assert.Empty(fixture.Runtime.GetRegisteredContractDescriptors());
        Assert.Collection(
            fixture.FreeCalls,
            call => AssertFree(call, 0, 8),
            call => AssertFree(call, 0, 4));
    }

    [Fact]
    public void DescriptorFailureDoesNotTransferBufferOwnership()
    {
        using IntrospectionRuntimeFixture fixture = new(IntrospectionMode.DescriptorFalse);

        Assert.Empty(fixture.Runtime.GetBundleDescriptors());
        Assert.Empty(fixture.Runtime.GetRegisteredContractDescriptors());
        Assert.Equal(3, fixture.CallbackOwnedBufferCount);
        Assert.Equal(2, fixture.PoisonedNativeBufferCount);
        Assert.Collection(
            fixture.FreeCalls,
            call => AssertFree(call, sizeof(ulong), sizeof(ulong)),
            call => AssertFree(call, Marshal.SizeOf<GuestContractHandle>(), 4));
    }

    [Fact]
    public void OlderRuntimeWithoutIntrospectionReturnsEmptySnapshots()
    {
        using IntrospectionRuntimeFixture fixture = new(IntrospectionMode.OlderRuntime);

        Assert.Empty(fixture.Runtime.GetBundleDescriptors());
        Assert.Empty(fixture.Runtime.GetRegisteredContractDescriptors());
        Assert.Empty(fixture.FreeCalls);
    }

    [Fact]
    public void DisposalInvalidatesIntrospectionSnapshots()
    {
        using IntrospectionRuntimeFixture fixture = new(IntrospectionMode.Empty);

        fixture.Runtime.Dispose();

        Assert.Throws<ObjectDisposedException>(() => fixture.Runtime.GetBundleDescriptors());
        Assert.Throws<ObjectDisposedException>(() => fixture.Runtime.GetRegisteredContractDescriptors());
        Assert.Throws<ObjectDisposedException>(() => _ = fixture.Runtime.HostHandle);
    }

    private static void AssertBundle(
        BundleDescriptor bundle,
        ulong expectedId,
        string expectedName,
        BundleSourceKind expectedSourceKind)
    {
        Assert.Equal(expectedId, bundle.Id);
        Assert.Equal(expectedName, bundle.Name);
        Assert.Equal(expectedSourceKind, bundle.SourceKind);
        Assert.Equal((uint)1, bundle.Version.Major);
        Assert.Equal(SupportedLanguage.Dotnet, bundle.Runtime);
    }

    private static void AssertContract(
        RegisteredContractDescriptor contract,
        uint expectedIndex,
        ulong expectedBundleId,
        ulong expectedContractId,
        string expectedPluginName,
        string expectedContractName)
    {
        Assert.Equal(expectedIndex, contract.Handle.Index);
        Assert.Equal(expectedBundleId, contract.BundleId);
        Assert.Equal(expectedContractId, contract.ContractId);
        Assert.Equal(expectedPluginName, contract.PluginName);
        Assert.Equal(expectedContractName, contract.ContractName);
        Assert.Equal((uint)1, contract.Version.Major);
    }

    private static void AssertFree(FreeCall call, int expectedSize, int expectedAlignment)
    {
        Assert.Equal((nuint)expectedSize, call.Size);
        Assert.Equal((nuint)expectedAlignment, call.Alignment);
    }

    private enum IntrospectionMode
    {
        Populated,
        Empty,
        OlderRuntime,
        DescriptorFalse,
    }

    private sealed class FreeCall
    {
        internal FreeCall(nint pointer, nuint size, nuint alignment)
        {
            Pointer = pointer;
            Size = size;
            Alignment = alignment;
        }

        internal nint Pointer { get; }

        internal nuint Size { get; }

        internal nuint Alignment { get; }
    }

    private sealed class OwnedByteBuffer
    {
        internal OwnedByteBuffer(nint pointer, nuint length, nuint alignment)
        {
            Pointer = pointer;
            Length = length;
            Alignment = alignment;
        }

        internal nint Pointer { get; }

        internal nuint Length { get; }

        internal nuint Alignment { get; }
    }

    private sealed class IntrospectionRuntimeFixture : IDisposable
    {
        private readonly List<Delegate> _delegates = [];
        private readonly nint _host;
        private readonly nint _introspection;
        private readonly IntrospectionMode _mode;
        private int _poisonedNativeBufferCount;
        private readonly List<nint> _callbackOwnedBuffers = [];
        private bool _disposed;

        internal IntrospectionRuntimeFixture(IntrospectionMode mode)
        {
            _mode = mode;
            nint nativeHost = Runtime.CreateNative();
            if (nativeHost == nint.Zero)
            {
                Runtime.ThrowLastError("Failed to create native runtime for introspection test.");
            }

            HostApi host = Marshal.PtrToStructure<HostApi>(nativeHost);
            if (mode != IntrospectionMode.OlderRuntime)
            {
                _introspection = CreateIntrospectionTable(mode);
                host.Reserved = _introspection;
                host.ListBundles = FunctionPointer(new Runtime.ListBundlesDelegate(ListBundles));
                host.Free = FunctionPointer(new Runtime.FreeDelegate(Free));
            }

            _host = Marshal.AllocHGlobal(Marshal.SizeOf<HostApi>());
            Marshal.StructureToPtr(host, _host, false);
            Runtime = new Runtime(_host, default, default);
        }

        internal Runtime Runtime { get; }

        internal List<FreeCall> FreeCalls { get; } = [];

        internal int PoisonedNativeBufferCount => _poisonedNativeBufferCount;

        internal int CallbackOwnedBufferCount => _callbackOwnedBuffers.Count;

        public void Dispose()
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
            Runtime.Dispose();
            ReleaseCallbackOwnedBuffers();
            Marshal.FreeHGlobal(_host);
            if (_introspection != nint.Zero)
            {
                Marshal.FreeHGlobal(_introspection);
            }
            GC.KeepAlive(_delegates);
        }

        private nint CreateIntrospectionTable(IntrospectionMode mode)
        {
            RuntimeIntrospection table = new()
            {
                GetBundleDescriptor = FunctionPointer(
                    new Runtime.GetBundleDescriptorDelegate(GetBundleDescriptor)),
                ListRegisteredGuestContracts = FunctionPointer(
                    new Runtime.ListRegisteredGuestContractsDelegate(ListRegisteredGuestContracts)),
                GetRegisteredContractDescriptor = FunctionPointer(
                    new Runtime.GetRegisteredContractDescriptorDelegate(GetRegisteredContractDescriptor)),
            };
            nint tablePointer = Marshal.AllocHGlobal(Marshal.SizeOf<RuntimeIntrospection>());
            Marshal.StructureToPtr(table, tablePointer, false);
            return tablePointer;
        }

        private void ListBundles(nint _, out Polyplug.Abi.Array bundles)
        {
            bundles = _mode switch
            {
                IntrospectionMode.Populated => AllocateUlongs([10, 20, 30, 40]),
                IntrospectionMode.DescriptorFalse => AllocateUlongs([99]),
                IntrospectionMode.Empty => AllocateEmptyArray(sizeof(ulong)),
                _ => default,
            };
        }

        private bool GetBundleDescriptor(nint _, ulong bundleId, out BundleDescriptorView descriptor)
        {
            if (_mode == IntrospectionMode.DescriptorFalse)
            {
                descriptor = Bundle(bundleId, "callback-owned", BundleSourceKind.Code);
                _callbackOwnedBuffers.Add(descriptor.Name);
                return false;
            }

            if (_mode != IntrospectionMode.Populated)
            {
                descriptor = default;
                return false;
            }

            descriptor = bundleId switch
            {
                10 => Bundle(10, "internal", BundleSourceKind.Internal),
                20 => Bundle(20, "path", BundleSourceKind.Path),
                30 => Bundle(30, "code", BundleSourceKind.Code),
                40 => Bundle(40, "bytes", BundleSourceKind.Bytes),
                _ => default,
            };
            return descriptor.Id != 0;
        }

        private void ListRegisteredGuestContracts(nint _, out Polyplug.Abi.Array handles)
        {
            handles = _mode switch
            {
                IntrospectionMode.Populated => AllocateHandles([
                    new GuestContractHandle { Index = 1, Generation = 7 },
                    new GuestContractHandle { Index = 2, Generation = 8 },
                ]),
                IntrospectionMode.DescriptorFalse => AllocateHandles([
                    new GuestContractHandle { Index = 99, Generation = 0 },
                ]),
                IntrospectionMode.Empty => AllocateEmptyArray(4),
                _ => default,
            };
        }

        private bool GetRegisteredContractDescriptor(
            nint _,
            GuestContractHandle handle,
            out RegisteredContractDescriptorView descriptor)
        {
            if (_mode == IntrospectionMode.DescriptorFalse)
            {
                descriptor = Contract(handle, 99, 999, "callback-owned", "callback.owned");
                _callbackOwnedBuffers.Add(descriptor.Plugin.Name);
                _callbackOwnedBuffers.Add(descriptor.Plugin.ContractName);
                return false;
            }

            if (_introspection == nint.Zero)
            {
                descriptor = default;
                return false;
            }

            descriptor = handle.Index switch
            {
                1 => Contract(handle, 11, 101, "alpha", "example.alpha"),
                2 => Contract(handle, 12, 102, "beta", "example.beta"),
                _ => default,
            };
            return descriptor.BundleId != 0;
        }

        private void Free(nint _, nint pointer, nuint size, nuint alignment)
        {
            FreeCalls.Add(new FreeCall(pointer, size, alignment));
            if (size != nuint.Zero)
            {
                int byteCount = checked((int)size);
                for (int index = 0; index < byteCount; index++)
                {
                    Marshal.WriteByte(pointer, index, 0xA5);
                }
            }

            _poisonedNativeBufferCount++;
            Marshal.FreeHGlobal(pointer);
        }

        private void ReleaseCallbackOwnedBuffers()
        {
            foreach (nint pointer in _callbackOwnedBuffers)
            {
                bool freedByRuntime = false;
                foreach (FreeCall call in FreeCalls)
                {
                    if (call.Pointer == pointer)
                    {
                        freedByRuntime = true;
                        break;
                    }
                }

                if (!freedByRuntime)
                {
                    Marshal.FreeHGlobal(pointer);
                }
            }
        }

        private BundleDescriptorView Bundle(ulong id, string name, BundleSourceKind sourceKind)
        {
            OwnedByteBuffer bytes = AllocateOwnedBytes(name);
            return new BundleDescriptorView
            {
                Id = id,
                Name = bytes.Pointer,
                NameLen = bytes.Length,
                NameAlign = bytes.Alignment,
                Version = new Polyplug.Abi.Version { Major = 1, Minor = 2, Patch = 3 },
                Runtime = SupportedLanguage.Dotnet,
                SourceKind = sourceKind,
            };
        }

        private RegisteredContractDescriptorView Contract(
            GuestContractHandle handle,
            ulong bundleId,
            ulong contractId,
            string pluginName,
            string contractName)
        {
            OwnedByteBuffer pluginNameBytes = AllocateOwnedBytes(pluginName);
            OwnedByteBuffer contractNameBytes = AllocateOwnedBytes(contractName);
            return new RegisteredContractDescriptorView
            {
                Handle = handle,
                BundleId = bundleId,
                ContractId = contractId,
                Plugin = new OwnedPluginDescriptorView
                {
                    Name = pluginNameBytes.Pointer,
                    NameLen = pluginNameBytes.Length,
                    NameAlign = pluginNameBytes.Alignment,
                    ContractName = contractNameBytes.Pointer,
                    ContractNameLen = contractNameBytes.Length,
                    ContractNameAlign = contractNameBytes.Alignment,
                    Version = new Polyplug.Abi.Version { Major = 1, Minor = 0, Patch = 0 },
                },
            };
        }

        private static OwnedByteBuffer AllocateOwnedBytes(string value)
        {
            byte[] bytes = Encoding.UTF8.GetBytes(value);
            nint pointer = Marshal.AllocHGlobal(bytes.Length);
            Marshal.Copy(bytes, 0, pointer, bytes.Length);
            return new OwnedByteBuffer(pointer, (nuint)bytes.Length, 1);
        }

        private static Polyplug.Abi.Array AllocateUlongs(ulong[] values)
        {
            nint pointer = Marshal.AllocHGlobal(checked(values.Length * sizeof(ulong)));
            for (int index = 0; index < values.Length; index++)
            {
                Marshal.WriteInt64(pointer + index * sizeof(ulong), unchecked((long)values[index]));
            }
            return new Polyplug.Abi.Array
            {
                Items = pointer,
                Len = (nuint)values.Length,
                Align = (nuint)sizeof(ulong),
            };
        }

        private static Polyplug.Abi.Array AllocateHandles(GuestContractHandle[] handles)
        {
            int stride = Marshal.SizeOf<GuestContractHandle>();
            nint pointer = Marshal.AllocHGlobal(checked(handles.Length * stride));
            for (int index = 0; index < handles.Length; index++)
            {
                Marshal.StructureToPtr(handles[index], pointer + index * stride, false);
            }
            return new Polyplug.Abi.Array
            {
                Items = pointer,
                Len = (nuint)handles.Length,
                Align = 4,
            };
        }

        private static Polyplug.Abi.Array AllocateEmptyArray(int alignment)
        {
            return new Polyplug.Abi.Array
            {
                Items = Marshal.AllocHGlobal(1),
                Len = 0,
                Align = (nuint)alignment,
            };
        }

        private nint FunctionPointer(Delegate callback)
        {
            _delegates.Add(callback);
            return Marshal.GetFunctionPointerForDelegate(callback);
        }

    }
}
