// Init.cs — Plugin initialization
using Polyplug.Guest;
using Reporter;

public static class PluginInit
{
    public static void Initialize()
    {
        var reporter = new ReporterPlugin();
        CsharpReporterVtables.SetCsharpReporterImpl(reporter);
    }
}
