using System.Runtime.InteropServices;
using Polyplug.Abi;

namespace Polyplug.Guest;

public static class HostVTableStorage
{
    private static IntPtr s_hostVTable = IntPtr.Zero;

    public static void StoreHostVTable(IntPtr hostVTable)
    {
        s_hostVTable = hostVTable;
    }

    public static IntPtr GetHostVTable()
    {
        return s_hostVTable;
    }
}