package = "polyplug-loader-dotnet"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug .NET/C# bundle loader for LuaJIT hosts",
   detailed = [[
      Loader that lets a polyplug LuaJIT host load .NET/C# plugin bundles.
      Bundled with the prebuilt libpolyplug_dotnet loader for Linux, macOS,
      and Windows.
   ]],
   homepage = "https://github.com/polyplug/polyplug",
   license = "MIT",
}

dependencies = {
   "lua >= 5.1",
   "polyplug",
}

build = {
   type = "builtin",
   modules = {
      ["polyplug.loaders.dotnet"] = "polyplug/loaders/dotnet.lua",
   },
   install = {
      lua = {
         ["polyplug_dotnet_linux"] = "_native/linux-x64/libpolyplug_dotnet.so",
         ["polyplug_dotnet_macos"] = "_native/macos-arm64/libpolyplug_dotnet.dylib",
         ["polyplug_dotnet_windows"] = "_native/windows-x64/polyplug_dotnet.dll",
      },
   },
}
