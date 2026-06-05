using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Reporter;

public sealed class ReporterPlugin : IDataReporterGuestContract
{
    public StringView Report(StringView input)
    {
        string s = StringHelpers.StripPrefix(input, "TRANSFORMED:");
        string[] parts = s.Split('|');
        if (parts.Length >= 3)
        {
            return PolyplugHost.AllocString($"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}");
        }
        return PolyplugHost.AllocString("INVALID:format");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        ReporterInterfaces.SetReporterImpl(new ReporterPlugin());
    }
}
