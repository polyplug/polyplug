-- rockspec file
package = "polyplug-loaders-native"
version = "1.0-1"
source = {
   url = "git+https://github.com/user/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-native"
}
description = {
   summary = "Native loader for polyplug plugin runtime",
   license = "MIT"
}
dependencies = {
   "lua >= 5.1",
   "luajit >= 2.0"
}
build = {
   type = "builtin",
   modules = {
      ["polyplug.loaders.native"] = "polyplug/loaders/native.lua"
   }
}