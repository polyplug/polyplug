// Init.cs — Plugin initialization
using Polyplug.Guest;
using CsvDecoder;

public static class PluginInit
{
    public static void Initialize()
    {
        var decoder = new DecoderPlugin();
        CsharpDecoderVtables.SetCsharpDecoderImpl(decoder);
    }
}
