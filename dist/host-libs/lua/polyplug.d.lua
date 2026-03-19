--- @class PolyplugRuntime
--- @field _ptr ffi.cdata* Opaque pointer managed by LuaJIT GC
local Runtime = {}

--- Load a bundle from the given directory path.
--- @param path string Full path to the bundle directory
--- @return boolean true on success
function Runtime:load_bundle(path) end

--- Reload a bundle.
--- @param path string Full path to the bundle directory
--- @return boolean true on success
function Runtime:reload_bundle(path) end

--- Find the first plugin providing a contract.
--- @param contract_id ffi.cdata* uint64_t contract identifier
--- @param min_version? number Minimum version (0 = any)
--- @return ffi.cdata* packed handle (uint64_t), or NULL_HANDLE if not found
function Runtime:find_by_contract(contract_id, min_version) end

--- Find first plugin from a specific bundle.
--- @param bundle_id ffi.cdata* uint64_t bundle identifier
--- @param contract_id ffi.cdata* uint64_t contract identifier
--- @param min_version? number Minimum version (0 = any)
--- @return ffi.cdata* packed handle (uint64_t), or NULL_HANDLE if not found
function Runtime:find_by_bundle(bundle_id, contract_id, min_version) end

--- Find all plugins providing a contract.
--- @param contract_id ffi.cdata* uint64_t contract identifier
--- @param min_version? number Minimum version (0 = any)
--- @param cap? number Maximum handles to return (default 64)
--- @return table, number Table of packed handles and total count
function Runtime:find_all_by_contract(contract_id, min_version, cap) end

--- Resolve a packed handle to a Guard object.
--- @param packed_handle ffi.cdata* uint64_t packed handle
--- @return PolyplugGuard|nil, string? Guard on success, nil + error string on failure
function Runtime:resolve_plugin(packed_handle) end

--- Explicitly free the runtime before GC.
function Runtime:free() end

--- @class PolyplugGuard
--- @field _ptr ffi.cdata* Opaque pointer managed by LuaJIT GC
local Guard = {}

--- Get the raw vtable pointer.
--- @return ffi.cdata* opaque void* pointer to the plugin vtable
function Guard:vtable() end

--- Explicitly free the guard before GC.
function Guard:free() end

--- @class Polyplug
--- @field NULL_HANDLE ffi.cdata* u64::MAX sentinel for invalid handles
--- @field Runtime PolyplugRuntime Constructor table
local M = {}

--- Load the libpolyplug.so shared library.
--- Must be called before any other function.
--- @param so_path string Full filesystem path to libpolyplug.so
--- @return table ffi library object
function M.load_lib(so_path) end

--- Get the last error string from the runtime.
--- @return string The last error message, or empty string if no error
function M.last_error() end

return M
