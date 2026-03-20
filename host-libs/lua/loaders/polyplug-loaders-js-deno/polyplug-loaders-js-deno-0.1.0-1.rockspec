package = "polyplug-loaders-js-deno"
version = "0.1.0-1"
source = {
   url = "git+https://github.com/polyplug/polyplug.git",
   branch = "main",
   dir = "host-libs/lua/loaders/polyplug-loaders-js-deno"
}
description = {
   summary = "JavaScript (Deno) loader for polyplug plugin runtime - loads Deno/TS plugins",
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
      ["polyplug.loaders.js_deno"] = "polyplug/loaders/js_deno.lua"
   },
   copy_directories = { "_native" }
}