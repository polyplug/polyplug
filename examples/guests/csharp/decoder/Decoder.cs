using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Decoder;

public sealed class DecoderPlugin : IPipelineDecoderGuestContract
{
    // Host handle for this runtime, captured at instance creation.
    private readonly IntPtr _host;

    public DecoderPlugin(IntPtr host)
    {
        _host = host;
    }

    public StringView Decode(StringView input)
    {
        string s = StringViewHelper.ToString(input).Replace(',', '|');
        return PolyplugHost.AllocString(_host, $"DECODED:{s}");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        // The factory receives the HostApi pointer per created instance.
        DecoderInterfaces.SetDecoderFactory(host => new DecoderPlugin(host));
    }
}
