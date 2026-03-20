package = "polyplug-loaders-lua"
version = "0.1.0-1"
source = {
   url = "git+https://github.com/polyplug/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-lua"
}
description = {
   summary = "Lua loader for polyplug plugin runtime - loads LuaJIT plugins",
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
      ["polyplug.loaders.lua"] = "polyplug/loaders/lua.lua"
   },
   copy_directories = { "_native" }
}