// Plugin.cs — Decoder plugin. ZERO unsafe. Pure business logic.
using System.Text;
using Polyplug.Guest;

namespace CsvDecoder;

// Business logic — pure safe C#.
public static class DecoderImpl
{
    // pipeline.Decoder@1 contract ID
    public const ulong DECODER_CONTRACT_ID = 0x12F3C106B0C3DC1EUL;

    public static byte[] Decode(string input)
    {
        // Parse "name,value,42" → "DECODED:name|value|42"
        string[] parts = input.Split(',');
        string joined = string.Join("|", parts);
        string result = $"DECODED:{joined}";
        return Encoding.UTF8.GetBytes(result);
    }
}
