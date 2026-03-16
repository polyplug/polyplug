// Reporter plugin — implements data.Reporter@1
// Input:  "name,value,42"
// Output: "REPORTED:name|value|42"

using System.Text;
using Polyplug.Guest;
using static Polyplug.Guest.StringViewHelper;

namespace Reporter;

public class ReporterPlugin : IDataReporterPlugin
{
    public StringView Report(StringView data)
    {
        string dataStr = data.ToString();
        string pipeSeparated = dataStr.Replace(',', '|');
        string result = $"REPORTED:{pipeSeparated}";
        var (sv, _) = FromStringPinned(result);
        return sv;
    }
}
