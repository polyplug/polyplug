-- rockspec file
package = "polyplug-guest"
version = "0.1.0-1"
source = {
   url = "git+https://github.com/polyplug/polyplug.git",
   branch = "main",
   dir = "guest-libs/lua"
}
description = {
   summary = "Guest library for writing polyplug plugins in Lua",
   detailed = [[
      A guest library for writing polyplug plugins in Lua/LuaJIT.
      Provides utilities for plugin initialization, ABI bindings, and host communication.
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
      ["polyplug_guest"] = "polyplug_guest.lua"
   }
}