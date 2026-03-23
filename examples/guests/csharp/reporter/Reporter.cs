using System;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Reporter;

public static class Plugin
{
    public static StringView Report(StringView input)
    {
        var s = StringHelpers.StripPrefix(input, "TRANSFORMED:");
        var parts = s.Split('|');
        if (parts.Length >= 3)
        {
            return StringHelpers.AllocString($"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}");
        }
        return StringHelpers.AllocString("INVALID:format");
    }
}
