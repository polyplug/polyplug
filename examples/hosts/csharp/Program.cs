using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug;
using Polyplug.Loaders;

internal static class Program
{
    private const ulong TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFUL;
    private const ulong REPORTER_CONTRACT_ID = 0x81D41D43E511D297UL;

    private const ulong FNV_OFFSET = 0xCBF29CE484222325UL;
    private const ulong FNV_PRIME = 0x00000100000001B3UL;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError AbiFn(IntPtr argsPtr, IntPtr outPtr);

    private readonly struct GuestSpec
    {
        public GuestSpec(string dir, string bundleName, ulong contractId, string fnName)
        {
            Dir = dir;
            BundleName = bundleName;
            ContractId = contractId;
            FnName = fnName;
        }

        public string Dir { get; }
        public string BundleName { get; }
        public ulong ContractId { get; }
        public string FnName { get; }
    }

    private static readonly GuestSpec[] Guests = new GuestSpec[]
    {
        new GuestSpec("rust/decoder",           "rust_transformer",       TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("rust/reporter",          "rust_reporter",          REPORTER_CONTRACT_ID,    "report"),
        new GuestSpec("cpp/transformer",        "cpp_transformer",        TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("cpp/reporter",           "cpp_reporter",           REPORTER_CONTRACT_ID,    "report"),
        new GuestSpec("csharp/encoder",         "csharp_transformer",     TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("csharp/reporter",        "csharp_reporter",        REPORTER_CONTRACT_ID,    "report"),
        new GuestSpec("python/decoder",         "python_transformer",     TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("python/reporter",        "python_reporter",        REPORTER_CONTRACT_ID,    "report"),
        new GuestSpec("lua/transformer",        "lua_transformer",        TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("lua/reporter",           "lua_reporter",           REPORTER_CONTRACT_ID,    "report"),
        new GuestSpec("js_quickjs/transformer", "js_quickjs_transformer", TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("js_quickjs/reporter",    "js_quickjs_reporter",    REPORTER_CONTRACT_ID,    "report"),
        new GuestSpec("js_deno/transformer",    "js_deno_transformer",    TRANSFORMER_CONTRACT_ID, "transform"),
        new GuestSpec("js_deno/reporter",       "js_deno_reporter",       REPORTER_CONTRACT_ID,    "report"),
    };

    private static int Main()
    {
        try
        {
            string repoRoot = FindRepoRoot();
            ConfigureNativeResolver(repoRoot);

            Runtime runtime = Runtime.Builder().Init();

            runtime.RegisterNativeLoader();
            runtime.RegisterDotnetLoader();
            runtime.RegisterPythonLoader();
            runtime.RegisterLuaLoader();
            runtime.RegisterJsLoader();
            runtime.RegisterJsDenoLoader();

            foreach (GuestSpec guest in Guests)
            {
                string path = Path.Combine(repoRoot, "examples", "guests", guest.Dir.Replace('/', Path.DirectorySeparatorChar));
                runtime.LoadBundle(path);
            }

            foreach (GuestSpec guest in Guests)
            {
                CallGuest(runtime, guest);
            }

            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"error: {ex.Message}");
            return 1;
        }
    }

    private static unsafe void CallGuest(Runtime runtime, GuestSpec guest)
    {
        ulong bundleId = BundleId(guest.BundleName);
        ulong handle = runtime.FindByBundle(bundleId, guest.ContractId, 0u);
        if (handle == ulong.MaxValue)
        {
            throw new InvalidOperationException($"plugin not found: {guest.BundleName}");
        }

        PluginGuard guard = runtime.ResolvePlugin(handle);
        try
        {
            IntPtr vtablePtr = guard.GetVTable();
            if (vtablePtr == IntPtr.Zero)
            {
                throw new InvalidOperationException($"null vtable: {guest.BundleName}");
            }

            PluginVTable vtable = Marshal.PtrToStructure<PluginVTable>(vtablePtr);
            if (vtable.FunctionsPtr == IntPtr.Zero || vtable.FunctionCount == 0u)
            {
                throw new InvalidOperationException($"no functions: {guest.BundleName}");
            }

            byte[] inputBytes = Encoding.UTF8.GetBytes("hello");
            GCHandle inputPin = GCHandle.Alloc(inputBytes, GCHandleType.Pinned);
            try
            {
                Polyplug.StringView inputSv = new Polyplug.StringView
                {
                    Ptr = inputPin.AddrOfPinnedObject(),
                    Len = (ulong)inputBytes.Length,
                };

                Polyplug.StringView outputSv = Polyplug.StringView.Empty;

                IntPtr fnPtr = Marshal.ReadIntPtr(vtable.FunctionsPtr, 0);
                AbiFn func = Marshal.GetDelegateForFunctionPointer<AbiFn>(fnPtr);
                AbiError err = func((IntPtr)(&inputSv), (IntPtr)(&outputSv));

                if (err.Code != AbiConstants.ABI_OK)
                {
                    throw new InvalidOperationException($"call failed for {guest.Dir}: code {err.Code}");
                }

                string result = outputSv.ToString();
                string label = $"[{guest.Dir}]";
                Console.WriteLine($"{label,-30} {guest.FnName}(\"hello\") = \"{result}\"");
            }
            finally
            {
                if (inputPin.IsAllocated)
                {
                    inputPin.Free();
                }
            }
        }
        finally
        {
            guard.Dispose();
        }
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
            string[] knownLibs = { "polyplug", "polyplug_native", "polyplug_dotnet", "polyplug_python", "polyplug_lua", "polyplug_js", "polyplug_js_deno" };
            bool isKnown = false;
            foreach (string known in knownLibs)
            {
                if (string.Equals(name, known, StringComparison.Ordinal))
                {
                    isKnown = true;
                    break;
                }
            }

            if (!isKnown)
            {
                return IntPtr.Zero;
            }

            string? envPath = Environment.GetEnvironmentVariable("POLYPLUG_SO");
            if (string.Equals(name, "polyplug", StringComparison.Ordinal)
                && !string.IsNullOrWhiteSpace(envPath) && File.Exists(envPath))
            {
                return NativeLibrary.Load(envPath);
            }

            string soName = $"lib{name}.so";
            string debugPath = Path.Combine(repoRoot, "target", "debug", soName);
            if (File.Exists(debugPath))
            {
                return NativeLibrary.Load(debugPath);
            }

            return NativeLibrary.Load(soName);
        };

        NativeLibrary.SetDllImportResolver(typeof(Runtime).Assembly, resolver);
        NativeLibrary.SetDllImportResolver(System.Reflection.Assembly.GetExecutingAssembly(), resolver);
    }
}
