// Plugin.cs — csv_encoder showcase plugin. ZERO unsafe. Pure business logic.
using System.Text;
using Polyplug.Guest;

namespace CsvEncoder;

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
public static class CsvEncoderImpl
{
    // pipeline.encoder@1 contract ID
    public const ulong ENCODER_CONTRACT_ID = 0x12AD37F43386F752UL;
    // pipeline.decoder@1 contract ID (for cross-plugin lookup demo)
    public const ulong DECODER_CONTRACT_ID = 0x133E62ABD6E7D5BEUL;

    // Encode DataRecord to CSV bytes.
    public static byte[] EncodeToCsv(string name, string value, uint count)
    {
        string csv = $"name,value,count\n{name},{value},{count}\n";
        return Encoding.UTF8.GetBytes(csv);
    }
}
