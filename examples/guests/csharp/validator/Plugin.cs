// Validator plugin — implements pipeline.Validator@1
// Input:  "name,value,42"
// Output: "VALID:name,value,42" or error

using System.Text;
using Polyplug.Guest;
using static Polyplug.Guest.StringViewHelper;

namespace Validator;

public class ValidatorPlugin : IPipelineValidatorPlugin
{
    public StringView Validate(StringView data)
    {
        string dataStr = data.ToString();
        // Simple validation: check for 3 comma-separated fields
        string[] parts = dataStr.Split(',');
        if (parts.Length != 3)
        {
            throw new PluginException(AbiConstants.ABI_ERROR_GENERIC, "invalid format: expected 3 fields");
        }
        string result = $"VALID:{dataStr}";
        var (sv, _) = FromStringPinned(result);
        return sv;
    }
}
