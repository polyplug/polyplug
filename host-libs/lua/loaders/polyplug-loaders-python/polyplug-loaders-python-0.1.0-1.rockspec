package = "polyplug-loaders-python"
version = "0.1.0-1"
source = {
   url = "git+https://github.com/polyplug/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-python"
}
description = {
   summary = "Python loader for polyplug plugin runtime - loads Python plugins",
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
      ["polyplug.loaders.python"] = "polyplug/loaders/python.lua"
   },
   copy_directories = { "_native" }
}