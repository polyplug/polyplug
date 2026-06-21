package = "polyplug"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug host runtime library for LuaJIT",
   detailed = [[
      LuaJIT FFI host library for polyplug: load plugin bundles at runtime and
      call guest contracts through the frozen C ABI. Includes the Runtime class,
      the native-library resolver, and reload-phase definitions. Bundled with
      the prebuilt libpolyplug core runtime for Linux, macOS, and Windows.
   ]],
   homepage = "https://github.com/polyplug/polyplug",
   license = "MIT",
}

-- Requires LuaJIT (uses the FFI module); stock PUC-Lua is unsupported, so no
-- "lua >= X" constraint is declared here.
dependencies = {
   "polyplug-abi",
}

build = {
   type = "builtin",
   modules = {
      ["polyplug"] = "polyplug.lua",
      ["polyplug.native"] = "polyplug/native.lua",
      ["polyplug.runtime"] = "polyplug/runtime.lua",
      ["polyplug.reload_phase"] = "polyplug/reload_phase.lua",
   },
   install = {
      lua = {
         ["polyplug_core_linux"] = "_native/linux-x64/libpolyplug.so",
         ["polyplug_core_macos"] = "_native/macos-arm64/libpolyplug.dylib",
         ["polyplug_core_windows"] = "_native/windows-x64/polyplug.dll",
      },
   },
}
