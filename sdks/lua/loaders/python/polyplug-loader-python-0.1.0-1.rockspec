package = "polyplug-loader-python"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug Python bundle loader for LuaJIT hosts",
   detailed = [[
      Loader that lets a polyplug LuaJIT host load Python plugin bundles.
      Bundled with the prebuilt libpolyplug_python loader for Linux, macOS,
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
      ["polyplug.loaders.python"] = "polyplug/loaders/python.lua",
   },
   install = {
      lua = {
         ["polyplug_python_linux"] = "_native/linux-x64/libpolyplug_python.so",
         ["polyplug_python_macos"] = "_native/macos-arm64/libpolyplug_python.dylib",
         ["polyplug_python_windows"] = "_native/windows-x64/polyplug_python.dll",
      },
   },
}
