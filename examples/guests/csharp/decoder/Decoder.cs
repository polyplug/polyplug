using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Decoder;

public sealed class DecoderPlugin : IPipelineDecoderGuestContract
{
    public StringView Decode(StringView input)
    {
        string s = StringViewHelper.ToString(input).Replace(',', '|');
        return PolyplugHost.AllocString($"DECODED:{s}");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        DecoderInterfaces.SetDecoderImpl(new DecoderPlugin());
    }
}
