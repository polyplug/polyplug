using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Transformer;

public sealed class TransformerPlugin : IDataTransformerGuestContract
{
    // Host handle for this runtime, captured at instance creation.
    private readonly IntPtr _host;

    public TransformerPlugin(IntPtr host)
    {
        _host = host;
    }

    public StringView Transform(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "DECODED:");
        string[] parts = s.Split('|');
        if (parts.Length >= 3 && int.TryParse(parts[2], out int count))
        {
            string name = parts[0].ToUpper();
            string value = $"{parts[1]} (transformed)";
            return PolyplugHost.AllocString(_host, $"TRANSFORMED:{name}|{value}|{count + 1}");
        }
        return PolyplugHost.AllocString(_host, "INVALID:format");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        // The factory receives the HostApi pointer per created instance.
        TransformerInterfaces.SetTransformerFactory(host => new TransformerPlugin(host));
    }
}
