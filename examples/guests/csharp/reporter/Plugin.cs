// Plugin.cs — reporter plugin. ZERO unsafe. Pure business logic.
using System.Text;
using Polyplug.Guest;

namespace Reporter;

// DataRecord ABI struct: name(16) + value(16) + count(4) + _pad(4) = 40 bytes.
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
public struct DataRecord
{
    public StringView Name;
    public StringView Value;
    public uint Count;
    private uint _pad;
}

// Business logic — pure safe C#.
public static class ReporterImpl
{
    // pipeline.reporter@1 contract ID
    public const ulong REPORTER_CONTRACT_ID = 0xD50E539CAE219A15UL;

    // Produce a human-readable report line from a DataRecord.
    public static byte[] BuildReport(string name, string value, uint count)
    {
        string line = $"[REPORT] name={name} value={value} count={count}";
        return Encoding.UTF8.GetBytes(line);
    }
}
