package = "polyplug-loaders-js"
version = "1.0-1"
source = {
   url = "git+https://github.com/user/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-js"
}
description = {
   summary = "JavaScript loader for polyplug plugin runtime",
   license = "MIT"
}
dependencies = {
   "lua >= 5.1",
   "luajit >= 2.0"
}
build = {
   type = "builtin",
   modules = {
      ["polyplug.loaders.js"] = "polyplug/loaders/js.lua"
   }
}