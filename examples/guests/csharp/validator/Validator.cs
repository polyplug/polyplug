using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Validator;

public sealed class ValidatorPlugin : IPipelineValidatorGuestContract
{
    public StringView Validate(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "DECODED:");
        string[] parts = s.Split('|');
        if (parts.Length == 3 && !string.IsNullOrEmpty(parts[0]) && !string.IsNullOrEmpty(parts[1]) && int.TryParse(parts[2], out _))
        {
            return PolyplugHost.AllocString($"VALID:{s}");
        }
        return PolyplugHost.AllocString("INVALID:expected name|value|count");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        ValidatorInterfaces.SetValidatorImpl(new ValidatorPlugin());
    }
}
