-- Runtime metadata introspection tests using a native HostApi fixture.
--
-- Run from sdks/lua/host/tests with:
--   POLYPLUG_LUA_INTROSPECTION_FIXTURE=/path/to/fixture.so luajit test_runtime_introspection.lua

local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
package.path = script_dir .. "../?.lua;"
           .. script_dir .. "../../../../sdks/lua/abi/?.lua;"
           .. package.path

local ffi = require("ffi")
local runtime = require("polyplug.runtime")

ffi.cdef([[
    const HostApi* polyplug_lua_test_runtime_introspection_host(void);
    void polyplug_lua_test_runtime_introspection_mode(uint32_t mode);
    void polyplug_lua_test_runtime_introspection_reset(void);
    size_t polyplug_lua_test_runtime_introspection_free_count(void);
    size_t polyplug_lua_test_runtime_introspection_free_size(size_t index);
    size_t polyplug_lua_test_runtime_introspection_free_alignment(size_t index);
    size_t polyplug_lua_test_runtime_introspection_resolve_count(void);
    uint32_t polyplug_lua_test_runtime_introspection_resolved_index(size_t index);
    uint32_t polyplug_lua_test_runtime_introspection_resolved_generation(size_t index);
]])

local function assert_equal(expected, actual, message)
    assert(expected == actual,
        message .. ": expected " .. tostring(expected) .. ", got " .. tostring(actual))
end

local function assert_length(expected, value, message)
    assert_equal(expected, #value, message)
end

local fixture_path = assert(os.getenv("POLYPLUG_LUA_INTROSPECTION_FIXTURE"),
    "POLYPLUG_LUA_INTROSPECTION_FIXTURE must name the native test fixture")
local fixture_lib = ffi.load(fixture_path)
fixture_lib.polyplug_lua_test_runtime_introspection_reset()

local host = fixture_lib.polyplug_lua_test_runtime_introspection_host()
local tested_runtime = setmetatable({
    _host = host,
    _host_struct = host[0],
}, runtime.Runtime)

local bundles = tested_runtime:bundle_descriptors()
assert_length(4, bundles, "all bundle origins are returned")
for index, source_kind in ipairs({ 0, 1, 2, 3 }) do
    local bundle = bundles[index]
    assert_equal(index * 11, tonumber(bundle.id), "bundle id is copied")
    assert_equal(index, bundle.version.major, "bundle version is copied")
    assert_equal(source_kind, bundle.source_kind, "bundle source kind is copied")
    assert_equal(4, bundle.runtime, "bundle language is copied")
end
assert_equal("internal", bundles[1].name, "bundle name is copied")
assert_equal("bytes", bundles[4].name, "last bundle name is copied")

local contracts = tested_runtime:registered_contract_descriptors()
assert_length(2, contracts, "multiple registered contracts are returned")
assert_equal(3, contracts[1].handle.index, "first contract handle is copied")
assert_equal(7, contracts[1].handle.generation, "first contract generation is copied")
assert_equal(11, tonumber(contracts[1].bundle_id), "first contract bundle is copied")
assert_equal(100, tonumber(contracts[1].contract_id), "first contract id is copied")
assert_equal("provider-1", contracts[1].plugin.name, "first provider name is copied")
assert_equal("example.contract.2", contracts[2].plugin.contract_name,
    "second provider contract name is copied")
assert_equal(1, contracts[2].plugin.version.minor, "contract version is copied")

assert_equal(10, tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_count()),
    "every descriptor string and returned ABI array is freed exactly once")
for index, expected_size in ipairs({
    #"internal",
    #"path",
    #"code",
    #"bytes",
    4 * ffi.sizeof("uint64_t"),
    #"provider-1",
    #"example.contract.1",
    #"provider-2",
    #"example.contract.2",
    2 * ffi.sizeof("GuestContractHandle"),
}) do
    assert_equal(expected_size,
        tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_size(index - 1)),
        "introspection allocation free size is exact")
end

fixture_lib.polyplug_lua_test_runtime_introspection_mode(1)
assert_length(0, tested_runtime:registered_contract_descriptors(),
    "empty introspection table is safe")
assert_equal(11, tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_count()),
    "non-null empty array is freed exactly once")
assert_equal(0, tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_size(10)),
    "empty array free size is exact")

fixture_lib.polyplug_lua_test_runtime_introspection_mode(2)
assert_length(0, tested_runtime:bundle_descriptors(),
    "legacy host without introspection has no bundles")
assert_length(0, tested_runtime:registered_contract_descriptors(),
    "legacy host without introspection has no contracts")
assert_equal(11, tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_count()),
    "legacy host does not free arrays")

local free_count_before_find_all = tonumber(
    fixture_lib.polyplug_lua_test_runtime_introspection_free_count())
local found_handles = tested_runtime:find_all_guest_contracts(0xA11CEULL, 1)
assert_length(2, found_handles, "find_all returns both fixture handles")
assert_equal(17, tonumber(found_handles[1].index),
    "first find_all handle survives native backing-storage poisoning")
assert_equal(23, tonumber(found_handles[1].generation),
    "first find_all generation survives native backing-storage poisoning")
assert_equal(29, tonumber(found_handles[2].index),
    "second find_all handle survives native backing-storage poisoning")
assert_equal(31, tonumber(found_handles[2].generation),
    "second find_all generation survives native backing-storage poisoning")
assert_equal(free_count_before_find_all + 1,
    tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_count()),
    "find_all frees its native handle allocation exactly once")
assert_equal(2 * ffi.sizeof("GuestContractHandle"),
    tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_size(free_count_before_find_all)),
    "find_all frees the full native handle allocation")
assert_equal(ffi.alignof("GuestContractHandle"),
    tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_free_alignment(free_count_before_find_all)),
    "find_all frees the native handle allocation with its exact alignment")
assert(tested_runtime._host_struct.resolve_guest_contract(
    tested_runtime._host, found_handles[1]) ~= nil,
    "first copied find_all handle remains resolvable after native storage free")
assert(tested_runtime._host_struct.resolve_guest_contract(
    tested_runtime._host, found_handles[2]) ~= nil,
    "second copied find_all handle remains resolvable after native storage free")
assert_equal(2, tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_resolve_count()),
    "both copied handles resolve after native storage free")
assert_equal(17,
    tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_resolved_index(0)),
    "resolver receives the copied first handle index")
assert_equal(31,
    tonumber(fixture_lib.polyplug_lua_test_runtime_introspection_resolved_generation(1)),
    "resolver receives the copied second handle generation")

print("runtime introspection tests passed")
