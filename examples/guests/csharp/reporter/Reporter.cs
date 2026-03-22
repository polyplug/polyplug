using System;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Reporter;

public static class Plugin
{
    public static StringView Report(StringView input)
    {
        var s = StringHelpers.ToString(input);
        if (s.StartsWith("TRANSFORMED:")) s = s[12..];
        var parts = s.Split('|');
        if (parts.Length >= 3)
        {
            return StringHelpers.AllocString($"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}");
        }
        return StringHelpers.AllocString("INVALID:format");
    }
}
