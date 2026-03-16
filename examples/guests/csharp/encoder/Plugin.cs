// Encoder plugin — implements pipeline.Encoder@1
// Input:  "name|value|42"
// Output: "ENCODED:name,value,42"

using System.Text;
using Polyplug.Guest;
using static Polyplug.Guest.StringViewHelper;

namespace CsvEncoder;

public class EncoderPlugin : IPipelineEncoderPlugin
{
    public StringView Encode(StringView data)
    {
        string dataStr = data.ToString();
        string commaSeparated = dataStr.Replace('|', ',');
        string result = $"ENCODED:{commaSeparated}";
        var (sv, _) = FromStringPinned(result);
        return sv;
    }
}
