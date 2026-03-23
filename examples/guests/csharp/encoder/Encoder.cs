using System;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Encoder;

public static class Plugin
{
    public static StringView Encode(StringView input)
    {
        var s = StringHelpers.StripPrefix(input, "TRANSFORMED:");
        return StringHelpers.AllocString(s.Replace('|', ','));
    }
}
