package = "polyplug-loader-native"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug native (cdylib) bundle loader for LuaJIT hosts",
   detailed = [[
      Loader that lets a polyplug LuaJIT host load native cdylib plugin bundles.
      Supports hot-reload. Bundled with the prebuilt libpolyplug_native loader
      for Linux, macOS, and Windows.
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
      ["polyplug.loaders.native"] = "polyplug/loaders/native.lua",
   },
   install = {
      lua = {
         ["polyplug_native_linux"] = "_native/linux-x64/libpolyplug_native.so",
         ["polyplug_native_macos"] = "_native/macos-arm64/libpolyplug_native.dylib",
         ["polyplug_native_windows"] = "_native/windows-x64/polyplug_native.dll",
      },
   },
}
