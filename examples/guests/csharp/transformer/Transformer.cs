using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Transformer;

public sealed class TransformerPlugin : IDataTransformerGuestContract
{
    public StringView Transform(StringView input)
    {
        string s = StringViewHelper.StripPrefix(input, "DECODED:");
        string[] parts = s.Split('|');
        if (parts.Length >= 3 && int.TryParse(parts[2], out int count))
        {
            string name = parts[0].ToUpper();
            string value = $"{parts[1]} (transformed)";
            return PolyplugHost.AllocString($"TRANSFORMED:{name}|{value}|{count + 1}");
        }
        return PolyplugHost.AllocString("INVALID:format");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        TransformerInterfaces.SetTransformerImpl(new TransformerPlugin());
    }
}
