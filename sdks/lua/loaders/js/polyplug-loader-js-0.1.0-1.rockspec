package = "polyplug-loader-js"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug JavaScript (QuickJS) bundle loader for LuaJIT hosts",
   detailed = [[
      Loader that lets a polyplug LuaJIT host load JavaScript plugin bundles
      running on QuickJS, each in its own isolated VM. Supports hot-reload.
      Bundled with the prebuilt libpolyplug_js loader for Linux, macOS, and
      Windows.
   ]],
   homepage = "https://github.com/polyplug/polyplug",
   license = "MIT",
}

-- Requires LuaJIT (uses the FFI module); stock PUC-Lua is unsupported, so no
-- "lua >= X" constraint is declared here.
dependencies = {
   "polyplug",
}

build = {
   type = "builtin",
   modules = {
      ["polyplug.loaders.js"] = "polyplug/loaders/js.lua",
   },
   install = {
      lua = {
         ["polyplug_js_linux"] = "_native/linux-x64/libpolyplug_js.so",
         ["polyplug_js_macos"] = "_native/macos-arm64/libpolyplug_js.dylib",
         ["polyplug_js_windows"] = "_native/windows-x64/polyplug_js.dll",
      },
   },
}
