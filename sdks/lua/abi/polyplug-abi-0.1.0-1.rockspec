package = "polyplug-abi"
version = "0.1.0-1"

source = {
   url = "https://github.com/polyplug/polyplug/releases/download/v0.1.0/polyplug-lua-0.1.0.tar.gz",
   dir = "polyplug-lua-0.1.0",
}

description = {
   summary = "polyplug ABI type definitions for LuaJIT",
   detailed = [[
      LuaJIT FFI type definitions mirroring the frozen polyplug C ABI:
      HostApi, BundleInitContext, StringView, AbiError, AbiErrorCode and the
      built-in-type helpers. This is the shared foundation required by both the
      polyplug host library and the polyplug-guest plugin library.
   ]],
   homepage = "https://github.com/polyplug/polyplug",
   license = "MIT",
}

dependencies = {
   "lua >= 5.1",
}

build = {
   type = "builtin",
   modules = {
      ["abi"] = "abi.lua",
      ["polyplug_abi"] = "polyplug_abi.lua",
   },
}
