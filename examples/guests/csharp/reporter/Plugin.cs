// Plugin.cs — Reporter plugin. ZERO unsafe. Pure business logic.
using System.Text;
using Polyplug.Guest;

namespace Reporter;

// Business logic — pure safe C#.
public static class ReporterImpl
{
    // data.Reporter@1 contract ID
    public const ulong REPORTER_CONTRACT_ID = 0x81D41D43E511D297UL;

    public static byte[] Report(string value)
    {
        string result = $"csharp:report({value})";
        return Encoding.UTF8.GetBytes(result);
    }
}
