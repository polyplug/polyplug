using System.Runtime.InteropServices;
using Polyplug.Abi;

namespace Polyplug.Guest;

public static class RuntimeAbiStorage
{
    private static IntPtr s_runtimeAbi = IntPtr.Zero;

    public static void StoreRuntimeAbi(IntPtr runtimeAbi)
    {
        s_runtimeAbi = runtimeAbi;
    }

    public static IntPtr GetRuntimeAbi()
    {
        return s_runtimeAbi;
    }
}