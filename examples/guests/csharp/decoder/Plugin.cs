// Decoder plugin — implements pipeline.Decoder@1
// Input:  "name,value,42"
// Output: "DECODED:name|value|42"

using System.Text;
using Polyplug.Guest;
using static Polyplug.Guest.StringViewHelper;

namespace CsvDecoder;

public class DecoderPlugin : IPipelineDecoderPlugin
{
    public StringView Decode(StringView input)
    {
        // Parse "name,value,42" → "DECODED:name|value|42"
        string inputStr = input.ToString();
        string[] parts = inputStr.Split(',');
        string joined = string.Join("|", parts);
        string result = $"DECODED:{joined}";
        var (sv, handle) = FromStringPinned(result);
        // Note: In real implementation, would allocate via host and free handle after copy
        // For now, keeping handle alive (leak for demo)
        return sv;
    }
}
