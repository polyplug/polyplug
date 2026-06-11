using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Encoder;

public sealed class EncoderPlugin : IPipelineEncoderGuestContract
{
    // Host handle for this runtime, captured at instance creation.
    private readonly IntPtr _host;

    public EncoderPlugin(IntPtr host)
    {
        _host = host;
    }

    public StringView Encode(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "TRANSFORMED:");
        return PolyplugHost.AllocString(_host, s.Replace('|', ','));
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        // The factory receives the HostApi pointer per created instance.
        EncoderInterfaces.SetEncoderFactory(host => new EncoderPlugin(host));
    }
}
