package = "polyplug-guest"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug guest-side library for writing Lua plugins",
   detailed = [[
      LuaJIT FFI guest library for authoring polyplug plugin bundles in Lua.
      Provides the registration and dispatch helpers that generated guest
      bindings build on, threading the HostApi pointer and per-call arena
      through instances and call arguments (no VM globals).
   ]],
   homepage = "https://github.com/polyplug/polyplug",
   license = "MIT",
}

dependencies = {
   "lua >= 5.1",
   "polyplug-abi",
}

build = {
   type = "builtin",
   modules = {
      ["polyplug_guest"] = "polyplug_guest.lua",
   },
}
