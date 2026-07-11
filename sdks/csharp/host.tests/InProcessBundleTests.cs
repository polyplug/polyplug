using System;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Abi;
using Polyplug.Host;
using Xunit;

namespace Polyplug.Host.Tests;

public sealed unsafe class InProcessBundleTests
{
    [Fact]
    public void RegistrationIsAtomicAndResidentSurvivesGcUntilSuccessfulUnload()
    {
        using FakeHost host = new();
        Runtime runtime = new(host.Pointer, default, default);
        try
        {
            WeakReference residentReference;

            {
                RegistrationResident resident = new(contractCount: 2);
                residentReference = new WeakReference(resident);
                InProcessBundle bundle = new(resident.Registration, resident);

                ulong bundleId = runtime.RegisterInProcessBundle(bundle);

                Assert.Equal(FakeHost.BundleId, bundleId);
                Assert.Equal((nuint)2, FakeHost.CapturedRegistration.ContractCount);
                Assert.Equal(1, runtime.InProcessBundleCount);
            }

            Collect();
            Assert.True(residentReference.IsAlive);

            FakeHost.UnloadError = AbiErrorCode.Generic;
            Assert.Throws<InvalidOperationException>(() => runtime.UnloadBundle(FakeHost.BundleId));
            Assert.Equal(1, runtime.InProcessBundleCount);

            FakeHost.UnloadError = AbiErrorCode.Ok;
            runtime.UnloadBundle(FakeHost.BundleId);
            Assert.Equal(0, runtime.InProcessBundleCount);
        }
        finally
        {
            Detach(runtime);
        }
    }

    [Fact]
    public void FailedRegistrationDoesNotRetainResident()
    {
        using FakeHost host = new();
        Runtime runtime = new(host.Pointer, default, default);
        RegistrationResident resident = new(contractCount: 1);
        try
        {
            InProcessBundle bundle = new(resident.Registration, resident);
            FakeHost.RegisterError = AbiErrorCode.Generic;

            Assert.Throws<InvalidOperationException>(() => runtime.RegisterInProcessBundle(bundle));
            Assert.Equal(0, runtime.InProcessBundleCount);
            Assert.False(resident.IsDisposed);
            FakeHost.RegisterError = AbiErrorCode.Ok;
            ulong bundleId = runtime.RegisterInProcessBundle(bundle);
            Assert.Equal(FakeHost.BundleId, bundleId);
            Assert.Throws<InvalidOperationException>(() => runtime.RegisterInProcessBundle(bundle));
            runtime.UnloadBundle(bundleId);
        }
        finally
        {
            resident.Dispose();
            Detach(runtime);
        }
    }

    [Fact]
    public void GeneratedAdapterKeepsStatePerRuntimeAcrossGcAndUnloadFailure()
    {
        using FakeHost hostA = new();
        using FakeHost hostB = new();
        Runtime runtimeA = new(hostA.Pointer, default, default);
        Runtime runtimeB = new(hostB.Pointer, default, default);
        try
        {
            int callsA = 0;
            int callsB = 0;
            InProcessBundle bundleA = transformer.InProcessBundleFactory.CreateInProcessBundle(
                _ => new StatefulTransformer(() => callsA++));
            InProcessBundle bundleB = transformer.InProcessBundleFactory.CreateInProcessBundle(
                _ => new StatefulTransformer(() => callsB++));

            runtimeA.RegisterInProcessBundle(bundleA);
            InProcessBundleRegistration registrationA = FakeHost.CapturedRegistration;
            runtimeB.RegisterInProcessBundle(bundleB);
            InProcessBundleRegistration registrationB = FakeHost.CapturedRegistration;

            Collect();
            InvokeTransform(runtimeA.HostHandle, registrationA);
            InvokeTransform(runtimeB.HostHandle, registrationB);
            Assert.Equal(1, callsA);
            Assert.Equal(1, callsB);

            FakeHost.UnloadError = AbiErrorCode.Generic;
            Assert.Throws<InvalidOperationException>(() => runtimeA.UnloadBundle(FakeHost.BundleId));
            InvokeTransform(runtimeA.HostHandle, registrationA);
            Assert.Equal(2, callsA);

            FakeHost.UnloadError = AbiErrorCode.Ok;
            runtimeA.UnloadBundle(FakeHost.BundleId);
            runtimeB.UnloadBundle(FakeHost.BundleId);
        }
        finally
        {
            Detach(runtimeA);
            Detach(runtimeB);
        }
    }

    private static void InvokeTransform(nint host, InProcessBundleRegistration registration)
    {
        InProcessContractRegistration contract = Marshal.PtrToStructure<InProcessContractRegistration>(registration.Contracts);
        GuestContractInterface iface = Marshal.PtrToStructure<GuestContractInterface>(contract.Interface);
        var create = (delegate* unmanaged[Cdecl]<nint, VmLoaderData, nint, nint, GuestContractInstance*, void>)iface.CreateInstance;
        GuestContractInstance instance = default;
        create(iface.AdapterContext, default, host, nint.Zero, &instance);
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
        }
        finally
        {
            inputPin.Free();
        }

        var destroy = (delegate* unmanaged[Cdecl]<nint, VmLoaderData, nint, GuestContractInstance, void>)iface.DestroyInstance;
        destroy(iface.AdapterContext, default, host, instance);
    }

    private sealed class StatefulTransformer(Action increment) : IDataTransformerGuestContract
    {
        public StringView Transform(StringView input)
        {
            increment();
            return input;
        }
    }
    private static void Collect()
    {
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
    }

    private static void Detach(Runtime runtime)
    {
        typeof(Runtime).GetField("_host", BindingFlags.Instance | BindingFlags.NonPublic)!
            .SetValue(runtime, nint.Zero);
    }

    private sealed class RegistrationResident : IDisposable
    {
        private readonly ulong[] _dependencies;
        private readonly InProcessContractRegistration[] _contracts;
        private GCHandle _dependenciesPin;
        private GCHandle _contractsPin;

        internal RegistrationResident(int contractCount)
        {
            _dependencies = [0xD1UL];
            _contracts = new InProcessContractRegistration[contractCount];
            _dependenciesPin = GCHandle.Alloc(_dependencies, GCHandleType.Pinned);
            _contractsPin = GCHandle.Alloc(_contracts, GCHandleType.Pinned);
            Registration = new InProcessBundleRegistration
            {
                DependencyIds = _dependenciesPin.AddrOfPinnedObject(),
                DependencyCount = (nuint)_dependencies.Length,
                Contracts = _contractsPin.AddrOfPinnedObject(),
                ContractCount = (nuint)_contracts.Length,
            };
        }

        internal InProcessBundleRegistration Registration { get; }

        internal bool IsDisposed { get; private set; }

        public void Dispose()
        {
            if (IsDisposed)
            {
                return;
            }

            _dependenciesPin.Free();
            _contractsPin.Free();
            IsDisposed = true;
        }
    }

    private sealed class FakeHost : IDisposable
    {
        internal const ulong BundleId = 0xB0A7D1EUL;
        private readonly nint _memory;

        internal FakeHost()
        {
            RegisterError = AbiErrorCode.Ok;
            UnloadError = AbiErrorCode.Ok;
            CapturedRegistration = default;

            HostApi api = new();
            object boxed = api;
            foreach (FieldInfo field in typeof(HostApi).GetFields(BindingFlags.Public | BindingFlags.Instance))
            {
                if (field.FieldType == typeof(nint))
                {
                    field.SetValue(boxed, (nint)(delegate* unmanaged[Cdecl]<void>)&NeverCall);
                }
            }
            api = (HostApi)boxed;
            api.RegisterInProcessBundle = (nint)(delegate* unmanaged[Cdecl]<nint, InProcessBundleRegistration*, ulong*, AbiError*, void>)&Register;
            api.UnloadBundle = (nint)(delegate* unmanaged[Cdecl]<nint, ulong, AbiError*, void>)&Unload;

            _memory = Marshal.AllocHGlobal(Marshal.SizeOf<HostApi>());
            Marshal.StructureToPtr(api, _memory, false);
        }

        internal nint Pointer => _memory;
        internal static AbiErrorCode RegisterError { get; set; }
        internal static AbiErrorCode UnloadError { get; set; }
        internal static InProcessBundleRegistration CapturedRegistration { get; private set; }

        public void Dispose()
        {
            Marshal.DestroyStructure<HostApi>(_memory);
            Marshal.FreeHGlobal(_memory);
        }

        [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
        private static void NeverCall()
        {
        }

        [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
        private static void Register(nint host, InProcessBundleRegistration* registration, ulong* outBundleId, AbiError* outError)
        {
            _ = host;
            if (registration != null)
            {
                CapturedRegistration = *registration;
            }
            *outBundleId = BundleId;
            *outError = new AbiError { Code = (uint)RegisterError };
        }

        [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
        private static void Unload(nint host, ulong bundleId, AbiError* outError)
        {
            _ = host;
            _ = bundleId;
            *outError = new AbiError { Code = (uint)UnloadError };
        }
    }
}
