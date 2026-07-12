using System;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

using Polyplug.Abi;
using Polyplug.Host;

using Xunit;

namespace Polyplug.Host.Tests;

public sealed unsafe class InProcessBundleTests
{
    static InProcessBundleTests()
    {
        TestNativeLibraries.EnsureInstalled();
    }

    [Fact]
    public void RegistrationStagesCanonicalManifestAndKeepsStatePerRuntime()
    {
        Runtime runtimeA = new();
        Runtime runtimeB = new();
        try
        {
            int callsA = 0;
            int callsB = 0;
            InProcessBundle bundleA = transformer.InProcessBundleFactory.CreateInProcessBundle(
                _ => new StatefulTransformer(() => callsA++));
            InProcessBundle bundleB = transformer.InProcessBundleFactory.CreateInProcessBundle(
                _ => new StatefulTransformer(() => callsB++));

            ulong bundleIdA = runtimeA.RegisterInProcessBundle(bundleA);
            ulong bundleIdB = runtimeB.RegisterInProcessBundle(bundleB);

            Collect();
            InvokeTransform(runtimeA, ref callsA);
            InvokeTransform(runtimeB, ref callsB);

            runtimeA.UnloadBundle(bundleIdA);
            runtimeB.UnloadBundle(bundleIdB);
        }
        finally
        {
            GC.KeepAlive(runtimeA);
            GC.KeepAlive(runtimeB);
        }
    }

    [Fact]
    public void ContractRegistrationFailureAbortsStagingAndDoesNotTransferResident()
    {
        Runtime runtime = new();
        CommitFailureResident failedResident = new(failRegistration: true);
        try
        {
            InProcessBundle failedBundle = new(
                ReadTransformerManifest(),
                failedResident,
                failedResident.RegisterContracts);

            Assert.Throws<InvalidOperationException>(() => runtime.RegisterInProcessBundle(failedBundle));
            Assert.False(failedResident.IsDisposed);
            Assert.Equal(0, runtime.InProcessBundleCount);

            failedResident.AllowRegistration();
            ulong bundleId = runtime.RegisterInProcessBundle(failedBundle);
            runtime.UnloadBundle(bundleId);
        }
        finally
        {
            failedResident.Dispose();
            GC.KeepAlive(runtime);
        }
    }

    [Fact]
    public void CommitFailureDiscardsStagingWithoutAbortingTwice()
    {
        Runtime runtime = new();
        CommitFailureResident failedResident = new();
        try
        {
            string manifest = Encoding.UTF8.GetString(ReadTransformerManifest());
            string invalidManifest = manifest.Replace(
                "\"data.Transformer@1\" = 1",
                "\"data.Transformer@1\" = 2",
                StringComparison.Ordinal);
            Assert.NotEqual(manifest, invalidManifest);
            InProcessBundle failedBundle = new(
                Encoding.UTF8.GetBytes(invalidManifest),
                failedResident,
                failedResident.RegisterContracts);

            Assert.Throws<InvalidOperationException>(() => runtime.RegisterInProcessBundle(failedBundle));
            Assert.False(failedResident.IsDisposed);
            Assert.Equal(0, runtime.InProcessBundleCount);

            InProcessBundle succeedingBundle = CreateTransformerBundle(() => { });
            ulong bundleId = runtime.RegisterInProcessBundle(succeedingBundle);
            runtime.UnloadBundle(bundleId);
        }
        finally
        {
            failedResident.Dispose();
            GC.KeepAlive(runtime);
        }
    }

    private static InProcessBundle CreateTransformerBundle(Action increment)
    {
        return transformer.InProcessBundleFactory.CreateInProcessBundle(
            _ => new StatefulTransformer(increment));
    }

    private static byte[] ReadTransformerManifest()
    {
        return File.ReadAllBytes(Path.Combine(
            TestNativeLibraries.RepoRoot,
            "examples",
            "guests",
            "csharp",
            "transformer",
            "generated",
            "manifest.toml"));
    }

    private static void InvokeTransform(Runtime runtime, ref int calls)
    {
        GuestContractHandle handle = runtime.FindGuestContract(
            TransformerInterfaces.DATA_TRANSFORMER_CONTRACT_ID,
            minVersion: 1);
        nint interfacePtr = runtime.ResolveGuestContract(handle);
        Assert.NotEqual(nint.Zero, interfacePtr);

        GuestContractInterface iface = Marshal.PtrToStructure<GuestContractInterface>(interfacePtr);
        var create = (delegate* unmanaged[Cdecl]<nint, VmLoaderData, nint, nint, GuestContractInstance*, void>)iface.CreateInstance;
        GuestContractInstance instance = default;
        create(iface.AdapterContext, default, runtime.HostHandle, nint.Zero, &instance);
        Assert.NotEqual(nint.Zero, instance.Data);

        var dispatch = (delegate* unmanaged[Cdecl]<nint, GuestContractInstance, nint, nint, AbiError*, void>)Marshal.ReadIntPtr(iface.Dispatch.Native.Functions);
        byte[] bytes = [1];
        GCHandle inputPin = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try
        {
            StringView input = new() { Ptr = inputPin.AddrOfPinnedObject(), Len = (nuint)bytes.Length };
            StringView output = default;
            AbiError error = default;
            dispatch(iface.AdapterContext, instance, (nint)(&input), (nint)(&output), &error);
            Assert.Equal((uint)AbiErrorCode.Ok, error.Code);
            Assert.Equal((nuint)1, output.Len);
            Assert.Equal(1, calls);
        }
        finally
        {
            inputPin.Free();
        }

        var destroy = (delegate* unmanaged[Cdecl]<nint, VmLoaderData, nint, GuestContractInstance, void>)iface.DestroyInstance;
        destroy(iface.AdapterContext, default, runtime.HostHandle, instance);
    }

    private sealed class StatefulTransformer(Action increment) : IDataTransformerGuestContract
    {
        public StringView Transform(StringView input)
        {
            increment();
            return input;
        }
    }

    private sealed class CommitFailureResident : IDisposable

    {
        private readonly nint[] _functions;
        private readonly GuestContractInterface[] _interfaces;
        private readonly PluginDescriptor[] _descriptors;
        private readonly byte[] _pluginName = Encoding.UTF8.GetBytes("transformer");
        private readonly byte[] _contractName = Encoding.UTF8.GetBytes("data.Transformer@1");
        private readonly GCHandle[] _pins;

        private bool _failRegistration;

        internal CommitFailureResident(bool failRegistration = false)
        {
            _failRegistration = failRegistration;
            _functions = [(nint)(delegate* unmanaged[Cdecl]<nint, GuestContractInstance, nint, nint, AbiError*, void>)&Dispatch];
            _pins = new GCHandle[5];
            _pins[0] = GCHandle.Alloc(_functions, GCHandleType.Pinned);
            _interfaces =
            [
                new GuestContractInterface
                {
                    ContractId = 0x4775991362CD68EEUL,
                    ContractVersion = new Polyplug.Abi.Version { Major = 1, Minor = 0, Patch = 0 },
                    DispatchType = DispatchType.Native,
                    CreateInstance = (nint)(delegate* unmanaged[Cdecl]<nint, VmLoaderData, nint, nint, GuestContractInstance*, void>)&CreateInstance,
                    DestroyInstance = (nint)(delegate* unmanaged[Cdecl]<nint, VmLoaderData, nint, GuestContractInstance, void>)&DestroyInstance,
                    Dispatch = new DispatchMechanisms
                    {
                        Native = new NativeDispatch { FunctionCount = 1, Functions = _pins[0].AddrOfPinnedObject() },
                    },
                },
            ];
            _descriptors = new PluginDescriptor[1];
            _pins[1] = GCHandle.Alloc(_interfaces, GCHandleType.Pinned);
            _pins[2] = GCHandle.Alloc(_descriptors, GCHandleType.Pinned);
            _pins[3] = GCHandle.Alloc(_pluginName, GCHandleType.Pinned);
            _pins[4] = GCHandle.Alloc(_contractName, GCHandleType.Pinned);
            _descriptors[0] = new PluginDescriptor
            {
                Name = new StringView { Ptr = _pins[3].AddrOfPinnedObject(), Len = (nuint)_pluginName.Length },
                ContractName = new StringView { Ptr = _pins[4].AddrOfPinnedObject(), Len = (nuint)_contractName.Length },
                Version = new Polyplug.Abi.Version { Major = 1, Minor = 0, Patch = 0 },
            };
        }

        internal bool IsDisposed { get; private set; }

        internal void AllowRegistration()
        {
            _failRegistration = false;
        }

        internal unsafe AbiError RegisterContracts(nint hostPtr)
        {
            if (_failRegistration)
            {
                return new AbiError { Code = (uint)AbiErrorCode.Generic };
            }

            var host = (HostApi*)hostPtr;
            var register = (delegate* unmanaged[Cdecl]<nint, PluginDescriptor*, GuestContractInterface*, AbiError*, void>)host->RegisterGuestContract;
            AbiError error = default;
            register(
                hostPtr,
                (PluginDescriptor*)_pins[2].AddrOfPinnedObject(),
                (GuestContractInterface*)_pins[1].AddrOfPinnedObject(),
                &error);
            return error;
        }

        public void Dispose()
        {
            if (IsDisposed)
            {
                return;
            }

            IsDisposed = true;
            foreach (GCHandle pin in _pins)
            {
                if (pin.IsAllocated)
                {
                    pin.Free();
                }
            }
        }

        [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
        private static void CreateInstance(
            nint adapterContext,
            VmLoaderData loaderData,
            nint host,
            nint args,
            GuestContractInstance* outInstance)
        {
            _ = adapterContext;
            _ = loaderData;
            _ = host;
            _ = args;
            if (outInstance != null)
            {
                *outInstance = default;
            }
        }

        [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
        private static void DestroyInstance(
            nint adapterContext,
            VmLoaderData loaderData,
            nint host,
            GuestContractInstance instance)
        {
            _ = adapterContext;
            _ = loaderData;
            _ = host;
            _ = instance;
        }

        [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
        private static void Dispatch(
            nint adapterContext,
            GuestContractInstance instance,
            nint args,
            nint output,
            AbiError* outError)
        {
            _ = adapterContext;
            _ = instance;
            _ = args;
            _ = output;
            if (outError != null)
            {
                *outError = new AbiError { Code = (uint)AbiErrorCode.Ok };
            }
        }
    }

    private static void Collect()
    {
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
    }
}
