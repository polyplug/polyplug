using System;
using PolyplugGuest;

namespace Encoder;

public static class Plugin
{
    public static StringView Encode(StringView input)
    {
        var s = StringHelpers.ToString(input);
        if (s.StartsWith("TRANSFORMED:")) s = s[12..];
        return StringHelpers.AllocString(s.Replace('|', ','));
    }
}
