namespace Polyplug;

/// P/Invoke wrappers and runtime builder for hosting polyplug from C#.
/// Full implementation is out of scope for this epic (host-libs/csharp is infrastructure scaffold).
public sealed class Runtime {
    private Runtime() { }

    public static RuntimeBuilder Builder() => new RuntimeBuilder();
}

public sealed class RuntimeBuilder {
    public RuntimeBuilder PluginDir(string path) => this;
    public Runtime Init() => throw new NotImplementedException("polyplug host-libs/csharp: not yet implemented");
}
