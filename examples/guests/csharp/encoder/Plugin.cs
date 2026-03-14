// Plugin.cs — Transformer plugin. ZERO unsafe. Pure business logic.
using System.Text;
using Polyplug.Guest;

namespace CsvEncoder;

// Business logic — pure safe C#.
public static class TransformerImpl
{
    // data.Transformer@1 contract ID
    public const ulong TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFUL;

    public static byte[] Transform(string input)
    {
        string result = $"csharp:transform({input})";
        return Encoding.UTF8.GetBytes(result);
    }
}
