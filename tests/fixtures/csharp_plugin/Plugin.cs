// Plugin.cs — C# fixture: HAND-WRITTEN business logic. No pointer operations.
// Implements test.add@1.0 contract.
using Polyplug.Guest;

namespace CsharpPlugin;

// The arg-pack struct for test.add::add (two u32 params)
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
public struct TestAddContractAddArgs {
    public uint A;
    public uint B;
}

// The arg-pack struct for test.add::add_primitive
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
public struct TestAddContractAddPrimitiveArgs {
    public uint A;
    public uint B;
}

// Business logic implementation — pure safe C#, no pointers.
public static class TestAddImpl {
    private static uint _counter = 0;
    private static readonly byte[] VERSION_BYTES = "1.0"u8.ToArray();

    public static uint Add(uint a, uint b) => a + b;
    public static uint AddPrimitive(uint a, uint b) => a + b;
    public static byte[] GetVersionBytes() => VERSION_BYTES;
    public static void Reset() => _counter = 0;
}
