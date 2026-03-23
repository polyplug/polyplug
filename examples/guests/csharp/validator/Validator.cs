using System;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Validator;

public static class Plugin
{
    public static StringView Validate(StringView input)
    {
        var s = StringHelpers.StripPrefix(input, "DECODED:");
        var parts = s.Split('|');
        if (parts.Length == 3 && !string.IsNullOrEmpty(parts[0]) && !string.IsNullOrEmpty(parts[1]) && int.TryParse(parts[2], out _))
        {
            return StringHelpers.AllocString($"VALID:{s}");
        }
        return StringHelpers.AllocString("INVALID:expected name|value|count");
    }
}
