// examples/hosts/csharp/Program.cs
// C# host example using polyplugc-generated bindings.
//
// This host demonstrates the real-world polyplug pattern:
//   1. Generate host bindings: polyplugc --api api.toml --lang csharp --out generated/
//   2. Use generated contract IDs from Polyplug.Generated.ContractIds
//
// Zero hand-written contract IDs.

using System;
using Polyplug;
using Polyplug.Generated;

class Program {
    static int Main(string[] args) {
        string pluginPath = args.Length > 0 ? args[0] : "examples/plugins";
        Console.Error.WriteLine($"plugin directory: {pluginPath}");
        
        try {
            var runtime = Runtime.Builder()
                .PluginDir(pluginPath)
                .Init();
            
            Console.Error.WriteLine("Runtime created successfully.");
            
            Console.WriteLine("\n=== polyplug csharp host example ===");
            
            // Try to find plugins by generated contract IDs
            ulong decoderHandle = runtime.FindByContract(ContractIds.PIPELINE_DECODER_CONTRACT_ID, 0);
            if (decoderHandle != ulong.MaxValue) {
                Console.WriteLine("[csharp_decoder]               found decoder plugin");
            }
            
            ulong transformerHandle = runtime.FindByContract(ContractIds.DATA_TRANSFORMER_CONTRACT_ID, 0);
            if (transformerHandle != ulong.MaxValue) {
                Console.WriteLine("[csharp_transformer]           found transformer plugin");
            }
            
            ulong encoderHandle = runtime.FindByContract(ContractIds.PIPELINE_ENCODER_CONTRACT_ID, 0);
            if (encoderHandle != ulong.MaxValue) {
                Console.WriteLine("[csharp_encoder]               found encoder plugin");
            }
            
            ulong reporterHandle = runtime.FindByContract(ContractIds.DATA_REPORTER_CONTRACT_ID, 0);
            if (reporterHandle != ulong.MaxValue) {
                Console.WriteLine("[csharp_reporter]              found reporter plugin");
            }
            
            ulong validatorHandle = runtime.FindByContract(ContractIds.PIPELINE_VALIDATOR_CONTRACT_ID, 0);
            if (validatorHandle != ulong.MaxValue) {
                Console.WriteLine("[csharp_validator]             found validator plugin");
            }
            
            Console.WriteLine("\ncsharp pipeline complete");
            return 0;
            
        } catch (Exception ex) {
            Console.Error.WriteLine($"error: {ex.Message}");
            return 1;
        }
    }
}
