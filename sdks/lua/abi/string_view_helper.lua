--- polyplug_abi — String helper functions for StringView.
-- This module provides convenience functions for working with StringView
-- from the polyplug ABI.
--
-- @module polyplug_abi.string_view_helper
-- @license MIT

local ffi = require("ffi")

local M = {}

--- Convert StringView to Lua string.
-- @param sv StringView from polyplug ABI (ffi.cdata)
-- @return string Lua string (UTF-8), empty string if nil/empty
function M.to_str(sv)
    if not sv or not sv.ptr or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

--- Check if StringView starts with prefix.
-- @param sv StringView from polyplug ABI
-- @param prefix string Prefix string to check for
-- @return boolean True if the string starts with the prefix
function M.starts_with(sv, prefix)
    local s = M.to_str(sv)
    return s:sub(1, #prefix) == prefix
end

--- Check if StringView ends with suffix.
-- @param sv StringView from polyplug ABI
-- @param suffix string Suffix string to check for
-- @return boolean True if the string ends with the suffix
function M.ends_with(sv, suffix)
    local s = M.to_str(sv)
    if #suffix > #s then
        return false
    end
    return s:sub(-#suffix) == suffix
end

--- Strip prefix from StringView if present.
-- @param sv StringView from polyplug ABI
-- @param prefix string Prefix string to strip
-- @return string String with prefix removed if present, otherwise original
function M.strip_prefix(sv, prefix)
    local s = M.to_str(sv)
    if s:sub(1, #prefix) == prefix then
        return s:sub(#prefix + 1)
    end
    return s
end

--- Split StringView by delimiter.
-- @param sv StringView from polyplug ABI
-- @param delimiter string Delimiter string to split by (default: whitespace)
-- @return table Array of strings resulting from the split
function M.split(sv, delimiter)
    local s = M.to_str(sv)
    if s == "" then
        return {}
    end
    
    delimiter = delimiter or "%s+"
    local result = {}
    local pattern = "(.-)" .. delimiter .. "()"
    local last_pos = 1
    
    for part, pos in s:gmatch(pattern) do
        table.insert(result, part)
        last_pos = pos
    end
    
    -- Add the remaining part after the last delimiter
    table.insert(result, s:sub(last_pos))
    
    return result
end

return M