using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Encoder;

public sealed class EncoderPlugin : IPipelineEncoderGuestContract
{
    public StringView Encode(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "TRANSFORMED:");
        return PolyplugHost.AllocString(s.Replace('|', ','));
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        EncoderInterfaces.SetEncoderImpl(new EncoderPlugin());
    }
}
