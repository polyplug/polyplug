-- Re-export all types from the auto-generated abi module.
-- The auto-generated file is at sdks/lua/abi/abi.lua (per D-28).
-- This file provides the standard polyplug_abi require path.
local abi = require("polyplug.abi")
return abi
