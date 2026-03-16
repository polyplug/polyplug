using System;
using PolyplugGuest;

namespace Decoder;

public static class Plugin
{
    public static StringView Decode(StringView input)
    {
        var s = StringHelpers.ToString(input).Replace(',', '|');
        return StringHelpers.AllocString($"DECODED:{s}");
    }
}
