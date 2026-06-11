using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Reporter;

public sealed class ReporterPlugin : IDataReporterGuestContract
{
    // Host handle for this runtime, captured at instance creation.
    private readonly IntPtr _host;

    public ReporterPlugin(IntPtr host)
    {
        _host = host;
    }

    public StringView Report(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "TRANSFORMED:");
        string[] parts = s.Split('|');
        if (parts.Length >= 3)
        {
            return PolyplugHost.AllocString(_host, $"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}");
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
        ReporterInterfaces.SetReporterFactory(host => new ReporterPlugin(host));
    }
}
