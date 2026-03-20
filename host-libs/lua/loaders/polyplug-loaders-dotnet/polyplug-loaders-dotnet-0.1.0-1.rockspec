package = "polyplug-loaders-dotnet"
version = "0.1.0-1"
source = {
   url = "git+https://github.com/polyplug/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-dotnet"
}
description = {
   summary = ".NET loader for polyplug plugin runtime - loads .NET plugins",
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
      ["polyplug.loaders.dotnet"] = "polyplug/loaders/dotnet.lua"
   },
   copy_directories = { "_native" }
}