using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Validator;

public sealed class ValidatorPlugin : IPipelineValidatorGuestContract
{
    // Host handle for this runtime, captured at instance creation.
    private readonly IntPtr _host;

    public ValidatorPlugin(IntPtr host)
    {
        _host = host;
    }

    public StringView Validate(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "DECODED:");
        string[] parts = s.Split('|');
        if (parts.Length == 3 && !string.IsNullOrEmpty(parts[0]) && !string.IsNullOrEmpty(parts[1]) && int.TryParse(parts[2], out _))
        {
            return PolyplugHost.AllocString(_host, $"VALID:{s}");
        }
        return PolyplugHost.AllocString(_host, "INVALID:expected name|value|count");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        // The factory receives the HostApi pointer per created instance.
        ValidatorInterfaces.SetValidatorFactory(host => new ValidatorPlugin(host));
    }
}
