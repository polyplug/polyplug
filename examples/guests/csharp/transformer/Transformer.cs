using System;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Transformer;

public static class Plugin
{
    public static StringView Transform(StringView input)
    {
        var s = StringHelpers.StripPrefix(input, "DECODED:");
        var parts = s.Split('|');
        if (parts.Length >= 3 && int.TryParse(parts[2], out var count))
        {
            var name = parts[0].ToUpper();
            var value = $"{parts[1]} (transformed)";
            return StringHelpers.AllocString($"TRANSFORMED:{name}|{value}|{count + 1}");
        }
        return StringHelpers.AllocString("INVALID:format");
    }
}
