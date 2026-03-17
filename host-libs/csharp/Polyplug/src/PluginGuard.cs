using System;
using System.Runtime.InteropServices;

namespace Polyplug;

public struct PluginGuard : IDisposable
{
    private nint _handle;
    private GuardReleaser? _releaser;

    internal PluginGuard(nint handle)
    {
        _handle = handle;
        _releaser = new GuardReleaser(handle);
    }

    public nint GetVTable()
    {
        if (_handle == nint.Zero)
        {
            return nint.Zero;
        }

        return Runtime.GetVTablePtr(_handle);
    }

    public void Dispose()
    {
        _releaser?.Release();
        _releaser = null;
        _handle = nint.Zero;
    }

    private sealed class GuardReleaser
    {
        private nint _handle;

        public GuardReleaser(nint handle)
        {
            _handle = handle;
        }

        ~GuardReleaser()
        {
            Release();
        }

        public void Release()
        {
            if (_handle != nint.Zero)
            {
                Runtime.ReleaseGuard(_handle);
                _handle = nint.Zero;
            }
        }
    }
}

public static class PluginGuardExtensions
{
    private delegate uint PluginFnDelegate(nint inputPtr, nint outputPtr);

    public static string CallFunction(this PluginGuard guard, uint funcIdx, string input)
    {
        var vtablePtr = guard.GetVTable();
        if (vtablePtr == nint.Zero)
        {
            throw new InvalidOperationException("PluginGuard has no vtable");
        }

        var vtable = Marshal.PtrToStructure<PluginVTable>(vtablePtr);
        if (funcIdx >= vtable.FunctionCount)
        {
            throw new InvalidOperationException($"Function index {funcIdx} out of bounds");
        }

        nint funcsPtr = vtable.FunctionsPtr;
        var funcPtr = Marshal.ReadIntPtr(funcsPtr, (int)(funcIdx * nint.Size));
        var func = Marshal.GetDelegateForFunctionPointer<PluginFnDelegate>(funcPtr);

        var inputBytes = System.Text.Encoding.UTF8.GetBytes(input);
        var inputBuf = Marshal.AllocHGlobal(inputBytes.Length);
        Marshal.Copy(inputBytes, 0, inputBuf, inputBytes.Length);

        var inputSv = new StringView { Ptr = inputBuf, Len = (ulong)inputBytes.Length };
        var outputSv = new StringView();

        var inputSvPtr = Marshal.AllocHGlobal(Marshal.SizeOf<StringView>());
        Marshal.StructureToPtr(inputSv, inputSvPtr, false);
        var outputSvPtr = Marshal.AllocHGlobal(Marshal.SizeOf<StringView>());
        Marshal.StructureToPtr(outputSv, outputSvPtr, false);

        try
        {
            var errCode = func(inputSvPtr, outputSvPtr);
            outputSv = Marshal.PtrToStructure<StringView>(outputSvPtr);

            if (errCode == 0 && outputSv.Ptr != nint.Zero && outputSv.Len > 0)
            {
                var result = Marshal.PtrToStringUTF8(outputSv.Ptr, (int)outputSv.Len) ?? string.Empty;
                NativeMethods.PolyplugHostFree(outputSv.Ptr, (nuint)outputSv.Len, 1);
                return result;
            }

            throw new InvalidOperationException($"Plugin returned error code={errCode}");
        }
        finally
        {
            Marshal.FreeHGlobal(inputBuf);
            Marshal.FreeHGlobal(inputSvPtr);
            Marshal.FreeHGlobal(outputSvPtr);
        }
    }
}