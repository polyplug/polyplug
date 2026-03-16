// Init.cs — Plugin initialization
using Polyplug.Guest;
using Validator;

public static class PluginInit
{
    public static void Initialize()
    {
        var validator = new ValidatorPlugin();
        CsharpValidatorVtables.SetCsharpValidatorImpl(validator);
    }
}
