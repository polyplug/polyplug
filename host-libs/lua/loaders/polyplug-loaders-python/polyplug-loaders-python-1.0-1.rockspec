package = "polyplug-loaders-python"
version = "1.0-1"
source = {
   url = "git+https://github.com/user/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-python"
}
description = {
   summary = "Python loader for polyplug plugin runtime",
   license = "MIT"
}
dependencies = {
   "lua >= 5.1",
   "luajit >= 2.0"
}
build = {
   type = "builtin",
   modules = {
      ["polyplug.loaders.python"] = "polyplug/loaders/python.lua"
   }
}