-- rockspec file
package = "polyplug"
version = "1.0-1"
source = {
   url = "git+https://github.com/polyplug/polyplug.git",
   branch = "main",
   dir = "host-libs/lua"
}
description = {
   summary = "LuaJIT FFI host library for the polyplug plugin runtime",
   detailed = [[
      A LuaJIT FFI-based host library for loading and managing polyplug plugins.
      Provides runtime management, plugin discovery, and hot-reload capabilities.
   ]],
   license = "MIT",
   homepage = "https://github.com/polyplug/polyplug"
}
dependencies = {
   "lua >= 5.1",
   "luajit >= 2.0"
}
build = {
   type = "builtin",
   modules = {
      ["polyplug"] = "polyplug.lua",
      ["polyplug.runtime"] = "polyplug/runtime.lua",
      ["polyplug.runtime_config"] = "polyplug/runtime_config.lua",
      ["polyplug.reload_phase"] = "polyplug/reload_phase.lua",
      ["polyplug.scanner"] = "scanner.lua"
   }
}