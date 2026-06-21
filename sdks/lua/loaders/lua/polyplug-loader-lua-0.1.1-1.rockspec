package = "polyplug-loader-lua"
version = "0.1.1-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.1/polyplug-lua-0.1.1.tar.gz",
   dir = "polyplug-lua-0.1.1",
}

description = {
   summary = "polyplug Lua bundle loader for LuaJIT hosts",
   detailed = [[
      Loader that lets a polyplug LuaJIT host load Lua plugin bundles, each in
      its own isolated VM. Supports hot-reload. Bundled with the prebuilt
      libpolyplug_lua loader for Linux, macOS, and Windows.
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
      ["polyplug.loaders.lua"] = "polyplug/loaders/lua.lua",
   },
   install = {
      lua = {
         ["polyplug_lua_linux"] = "_native/linux-x64/libpolyplug_lua.so",
         ["polyplug_lua_macos"] = "_native/macos-arm64/libpolyplug_lua.dylib",
         ["polyplug_lua_windows"] = "_native/windows-x64/polyplug_lua.dll",
      },
   },
}
