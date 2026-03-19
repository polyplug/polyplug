package = "polyplug-loaders-lua"
version = "1.0-1"
source = {
   url = "git+https://github.com/user/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-lua"
}
description = {
   summary = "Lua loader for polyplug plugin runtime",
   license = "MIT"
}
dependencies = {
   "lua >= 5.1",
   "luajit >= 2.0"
}
build = {
   type = "builtin",
   modules = {
      ["polyplug.loaders.lua"] = "polyplug/loaders/lua.lua"
   }
}