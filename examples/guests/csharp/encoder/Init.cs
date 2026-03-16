// Init.cs — Plugin initialization
using Polyplug.Guest;
using CsvEncoder;

public static class PluginInit
{
    public static void Initialize()
    {
        var encoder = new EncoderPlugin();
        CsharpEncoderVtables.SetCsharpEncoderImpl(encoder);
    }
}
