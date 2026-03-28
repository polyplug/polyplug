using Polyplug.Guest;
using Polyplug.Abi;
using Polyplug.Generated;

public class WorkerImpl : ExampleWorkerPlugin
{
    public StringView DoWork(StringView input)
    {
        var logger = HostLoggerCaller.FromHost(AbiHelpers.GetHostVtable(), 1);

        if (logger != null && logger.IsValid)
        {
            var inputStr = StringHelpers.ToString(input);
            logger.Log($"Processing input: {inputStr}");
            logger.Log("Step 1: Analyzing input");
            logger.Log("Step 2: Transforming data");
            logger.Log("Step 3: Generating output");
        }

        var inputStr = StringHelpers.ToString(input);
        var result = $"WORKED: {inputStr.ToUpper()}";
        return StringHelpers.AllocString(result);
    }
}

public static class PluginInit
{
    public static void Initialize()
    {
        Contracts.SetWorkerImpl(new WorkerImpl());
    }
}