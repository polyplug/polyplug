local ffi = require("ffi")
local M = {}

M.POLYPLUG_ABI_VERSION = ffi.cast("uint32_t", 1)
local function fnv1a_64(&[u8] data) end

local function contract_id(&str name, uint32_t major) end

local function bundle_id(&str name) end

local function host_contract_id(&str name, uint32_t major) end

local function plugin_contract_id(&str name, uint32_t major) end

return M
