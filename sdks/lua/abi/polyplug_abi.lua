-- Re-export all types from the auto-generated abi module.
-- The auto-generated file is the sibling abi.lua (sdks/lua/abi/abi.lua, per D-28).
-- This file provides the standard polyplug_abi require path. Both files share the
-- same package.path directory, so the sibling resolves via require("abi").
local abi = require("abi")
return abi
