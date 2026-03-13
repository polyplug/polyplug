using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug;

internal static class Program
{
    private const ulong DECODER_CONTRACT_ID = 0x133E62ABD6E7D5BEUL;
    private const ulong TRANSFORMER_CONTRACT_ID = 0x0E3044133E12EB05UL;
    private const ulong ENCODER_CONTRACT_ID = 0x12AD37F43386F752UL;
    private const ulong REPORTER_CONTRACT_ID = 0xD50E539CAE219A15UL;
    private const ulong VALIDATOR_CONTRACT_ID = 0x027ABCEBF8020D90UL;

    private const ulong FNV_OFFSET = 0xCBF29CE484222325UL;
    private const ulong FNV_PRIME = 0x00000100000001B3UL;

    [StructLayout(LayoutKind.Sequential)]
    private struct DataRecord
    {
        public StringView Name;
        public StringView Value;
        public uint Count;
        public uint Pad;
    }

    [StructLayout(LayoutKind.Sequential)]
    private unsafe struct ValidationResult
    {
        public byte Valid;
        public fixed byte Pad[7];
        public StringView Reason;
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError AbiFn(IntPtr argsPtr, IntPtr outPtr);

    [DllImport("polyplug", EntryPoint = "polyplug_host_free", CallingConvention = CallingConvention.Cdecl)]
    private static extern void polyplug_host_free(IntPtr ptr, UIntPtr size, UIntPtr align);

    private sealed class PluginEntry : IDisposable
    {
        public PluginEntry(string name, PluginGuard guard, PluginVTable vtable)
        {
            Name = name;
            Guard = guard;
            VTable = vtable;
        }

        public string Name { get; }
        public PluginGuard Guard { get; private set; }
        public PluginVTable VTable { get; }

        public void Dispose()
        {
            Guard.Dispose();
            Guard = default;
        }
    }

    private static int Main()
    {
        try
        {
            string repoRoot = FindRepoRoot();
            ConfigureNativeResolver(repoRoot);

            Console.WriteLine("=== polyplug C# host example ===");

            Runtime runtime = Runtime.Builder().Init();
            List<string> bundles = BuildBundlePaths(repoRoot);
            LoadAllBundles(runtime, bundles);

            Dictionary<string, PluginEntry> plugins = ResolvePlugins(runtime);
            try
            {
                RunPipeline(
                    "Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator",
                    plugins["decoder_rust"],
                    plugins["transformer_cpp"],
                    plugins["encoder_rust"],
                    plugins["reporter_csharp"],
                    plugins["validator_cpp"],
                    "Alice,hello,3\n"
                );

                RunPipeline(
                    "Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator",
                    plugins["decoder_python"],
                    plugins["transformer_lua"],
                    plugins["encoder_csharp"],
                    plugins["reporter_python"],
                    plugins["validator_lua"],
                    "Bob,world,4\n"
                );

                RunPipeline(
                    "Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator",
                    plugins["decoder_rust"],
                    plugins["transformer_cpp"],
                    plugins["encoder_csharp"],
                    plugins["reporter_js"],
                    plugins["validator_js"],
                    "Cara,polyplug,5\n"
                );
            }
            finally
            {
                foreach (PluginEntry entry in plugins.Values)
                {
                    entry.Dispose();
                }
            }

            Console.WriteLine("pipeline complete");
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"error: {ex.Message}");
            return 1;
        }
    }

    private static string FindRepoRoot()
    {
        string[] seeds = { AppContext.BaseDirectory, Directory.GetCurrentDirectory() };
        foreach (string seed in seeds)
        {
            string dir = Path.GetFullPath(seed);
            for (int i = 0; i < 8; i++)
            {
                string examplesPath = Path.Combine(dir, "examples", "guests");
                if (Directory.Exists(examplesPath))
                {
                    return dir;
                }
                DirectoryInfo? parent = Directory.GetParent(dir);
                if (parent == null)
                {
                    break;
                }
                dir = parent.FullName;
            }
        }

        return Directory.GetCurrentDirectory();
    }

    private static void ConfigureNativeResolver(string repoRoot)
    {
        DllImportResolver resolver = (string name, System.Reflection.Assembly assembly, DllImportSearchPath? path) =>
        {
            if (!string.Equals(name, "polyplug", StringComparison.Ordinal))
            {
                return IntPtr.Zero;
            }

            string? envPath = Environment.GetEnvironmentVariable("POLYPLUG_SO");
            if (!string.IsNullOrWhiteSpace(envPath) && File.Exists(envPath))
            {
                return NativeLibrary.Load(envPath);
            }

            string fullPath = Path.Combine(
                repoRoot,
                "examples",
                "hosts",
                "js",
                "target",
                "debug",
                "libpolyplug_full.so"
            );
            if (File.Exists(fullPath))
            {
                return NativeLibrary.Load(fullPath);
            }

            string fallback = Path.Combine(repoRoot, "target", "debug", "libpolyplug.so");
            return NativeLibrary.Load(fallback);
        };

        NativeLibrary.SetDllImportResolver(typeof(Runtime).Assembly, resolver);
        NativeLibrary.SetDllImportResolver(System.Reflection.Assembly.GetExecutingAssembly(), resolver);
    }

    private static List<string> BuildBundlePaths(string repoRoot)
    {
        return new List<string>
        {
            Path.Combine(repoRoot, "examples", "guests", "rust", "decoder"),
            Path.Combine(repoRoot, "examples", "guests", "rust", "encoder"),
            Path.Combine(repoRoot, "examples", "guests", "cpp", "transformer"),
            Path.Combine(repoRoot, "examples", "guests", "cpp", "validator"),
            Path.Combine(repoRoot, "examples", "guests", "csharp", "encoder"),
            Path.Combine(repoRoot, "examples", "guests", "csharp", "reporter"),
            Path.Combine(repoRoot, "examples", "guests", "python", "decoder"),
            Path.Combine(repoRoot, "examples", "guests", "python", "reporter"),
            Path.Combine(repoRoot, "examples", "guests", "lua", "transformer"),
            Path.Combine(repoRoot, "examples", "guests", "lua", "validator"),
            Path.Combine(repoRoot, "examples", "guests", "js", "validator"),
            Path.Combine(repoRoot, "examples", "guests", "js", "reporter"),
        };
    }

    private static void LoadAllBundles(Runtime runtime, List<string> bundles)
    {
        Console.WriteLine("Loading 12 guest plugins...");
        int index = 0;
        foreach (string path in bundles)
        {
            index++;
            runtime.LoadBundle(path);
            Console.WriteLine($"  [OK]  {index,2}/12 {Path.GetFileName(Path.GetDirectoryName(path) ?? path)}/{Path.GetFileName(path)}");
        }
    }

    private static Dictionary<string, PluginEntry> ResolvePlugins(Runtime runtime)
    {
        Dictionary<string, PluginEntry> plugins = new Dictionary<string, PluginEntry>(StringComparer.Ordinal)
        {
            { "decoder_rust", ResolveByBundle(runtime, "csv_decoder", DECODER_CONTRACT_ID) },
            { "encoder_rust", ResolveByBundle(runtime, "csv_encoder_rust", ENCODER_CONTRACT_ID) },
            { "transformer_cpp", ResolveByBundle(runtime, "uppercase_transformer", TRANSFORMER_CONTRACT_ID) },
            { "validator_cpp", ResolveByBundle(runtime, "cpp_validator", VALIDATOR_CONTRACT_ID) },
            { "encoder_csharp", ResolveByBundle(runtime, "csv_encoder_csharp", ENCODER_CONTRACT_ID) },
            { "reporter_csharp", ResolveByBundle(runtime, "csharp_reporter", REPORTER_CONTRACT_ID) },
            { "decoder_python", ResolveByBundle(runtime, "python_decoder", DECODER_CONTRACT_ID) },
            { "reporter_python", ResolveByBundle(runtime, "summary_reporter", REPORTER_CONTRACT_ID) },
            { "transformer_lua", ResolveByBundle(runtime, "reverse_transformer", TRANSFORMER_CONTRACT_ID) },
            { "validator_lua", ResolveByBundle(runtime, "lua_validator", VALIDATOR_CONTRACT_ID) },
            { "validator_js", ResolveByBundle(runtime, "field_validator", VALIDATOR_CONTRACT_ID) },
            { "reporter_js", ResolveByBundle(runtime, "js_reporter", REPORTER_CONTRACT_ID) },
        };

        return plugins;
    }

    private static PluginEntry ResolveByBundle(Runtime runtime, string bundleName, ulong contractId)
    {
        ulong bundleId = BundleId(bundleName);
        ulong handle = runtime.FindByBundle(bundleId, contractId, 0u);
        if (handle == ulong.MaxValue)
        {
            throw new InvalidOperationException($"plugin not found for bundle: {bundleName}");
        }

        PluginGuard guard = runtime.ResolvePlugin(handle);
        IntPtr vtablePtr = guard.GetVTable();
        if (vtablePtr == IntPtr.Zero)
        {
            guard.Dispose();
            throw new InvalidOperationException($"null vtable for bundle: {bundleName}");
        }

        PluginVTable vtable = Marshal.PtrToStructure<PluginVTable>(vtablePtr);
        return new PluginEntry(bundleName, guard, vtable);
    }

    private static ulong BundleId(string name)
    {
        byte[] data = Encoding.UTF8.GetBytes(name);
        ulong hash = FNV_OFFSET;
        foreach (byte b in data)
        {
            hash ^= b;
            hash *= FNV_PRIME;
        }

        return hash;
    }

    private static unsafe void RunPipeline(
        string label,
        PluginEntry decoder,
        PluginEntry transformer,
        PluginEntry encoder,
        PluginEntry reporter,
        PluginEntry validator,
        string inputCsv
    )
    {
        Console.WriteLine($"--- {label} ---");
        byte[] inputBytes = Encoding.UTF8.GetBytes(inputCsv);
        GCHandle inputHandle = GCHandle.Alloc(inputBytes, GCHandleType.Pinned);
        try
        {
            Polyplug.Buffer inputBuf = new Polyplug.Buffer
            {
                Ptr = inputHandle.AddrOfPinnedObject(),
                Len = (ulong)inputBytes.Length,
                Cap = (ulong)inputBytes.Length,
            };

            DataRecord record = new DataRecord
            {
                Name = StringView.Empty,
                Value = StringView.Empty,
                Count = 0u,
                Pad = 0u,
            };

            AbiError decodeErr = CallFn(decoder.VTable, 0, (IntPtr)(&inputBuf), (IntPtr)(&record));
            EnsureOk(decodeErr, "decode");

            DataRecord transformed = new DataRecord
            {
                Name = StringView.Empty,
                Value = StringView.Empty,
                Count = 0u,
                Pad = 0u,
            };

            AbiError transformErr = CallFn(transformer.VTable, 0, (IntPtr)(&record), (IntPtr)(&transformed));
            EnsureOk(transformErr, "transform");

            Polyplug.Buffer encoded = new Polyplug.Buffer
            {
                Ptr = IntPtr.Zero,
                Len = 0ul,
                Cap = 0ul,
            };

            AbiError encodeErr = CallFn(encoder.VTable, 0, (IntPtr)(&transformed), (IntPtr)(&encoded));
            EnsureOk(encodeErr, "encode");

            string output = ReadBuffer(encoded).TrimEnd();
            Console.WriteLine($"Run output: {output}");

            StringView reportSv = StringView.Empty;
            AbiError reportErr = CallFn(reporter.VTable, 0, (IntPtr)(&transformed), (IntPtr)(&reportSv));
            EnsureOk(reportErr, "report");
            string report = reportSv.ToString();
            if (!string.IsNullOrWhiteSpace(report))
            {
                Console.WriteLine($"Run summary: {report}");
            }

            ValidationResult validation = default;
            AbiError validateErr = CallFn(validator.VTable, 0, (IntPtr)(&transformed), (IntPtr)(&validation));
            EnsureOk(validateErr, "validate");
            string reason = validation.Reason.ToString();
            Console.WriteLine($"Validation: {(validation.Valid == 0 ? "invalid" : "ok")} ({reason})");
        }
        finally
        {
            if (inputHandle.IsAllocated)
            {
                inputHandle.Free();
            }
        }
    }

    private static AbiError CallFn(PluginVTable vtable, int fnId, IntPtr argsPtr, IntPtr outPtr)
    {
        if (vtable.FunctionsPtr == IntPtr.Zero)
        {
            return new AbiError { Code = AbiConstants.ABI_ERROR_GENERIC, Message = StringView.Empty };
        }

        if (fnId < 0 || fnId >= vtable.FunctionCount)
        {
            return new AbiError { Code = AbiConstants.ABI_ERROR_GENERIC, Message = StringView.Empty };
        }

        IntPtr fnPtr = Marshal.ReadIntPtr(vtable.FunctionsPtr, fnId * IntPtr.Size);
        AbiFn func = Marshal.GetDelegateForFunctionPointer<AbiFn>(fnPtr);
        return func(argsPtr, outPtr);
    }

    private static void EnsureOk(AbiError err, string stage)
    {
        if (err.Code == AbiConstants.ABI_OK)
        {
            return;
        }

        string message = err.Message.ToString();
        FreeStringView(err.Message);
        if (string.IsNullOrWhiteSpace(message))
        {
            message = "unknown error";
        }

        throw new InvalidOperationException($"{stage} failed: {message} (code {err.Code})");
    }

    private static string ReadBuffer(Polyplug.Buffer buffer)
    {
        if (buffer.Ptr == IntPtr.Zero || buffer.Len == 0ul)
        {
            return string.Empty;
        }

        int length = buffer.Len > int.MaxValue ? int.MaxValue : (int)buffer.Len;
        return Marshal.PtrToStringUTF8(buffer.Ptr, length) ?? string.Empty;
    }

    private static void FreeBuffer(Polyplug.Buffer buffer)
    {
        if (buffer.Ptr == IntPtr.Zero || buffer.Len == 0ul)
        {
            return;
        }

        ulong size = buffer.Cap != 0ul ? buffer.Cap : buffer.Len;
        polyplug_host_free(buffer.Ptr, (UIntPtr)size, (UIntPtr)1u);
    }

    private static void FreeStringView(StringView view)
    {
        if (view.Ptr == IntPtr.Zero || view.Len == 0ul)
        {
            return;
        }

        polyplug_host_free(view.Ptr, (UIntPtr)view.Len, (UIntPtr)1u);
    }
}
