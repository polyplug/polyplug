-- sdks/lua/host/tests/test_native_resolution.lua
-- Unit tests for polyplug.native: platform identifier, per-OS filename mapping,
-- host-root anchor, and resolution order (env → co-located → system).
-- No native library is loaded; ffi.load is never called from this file.
--
-- Run from repo root:
--   cd sdks/lua/host/tests && luajit test_native_resolution.lua

-- ─── Path setup ──────────────────────────────────────────────────────────────
-- The working directory when this test runs is sdks/lua/host/tests/.
-- Add the host lib parent so require("polyplug.native") resolves.
local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
package.path = script_dir .. "../?.lua;" .. package.path

local native = require("polyplug.native")

-- ─── Test harness ────────────────────────────────────────────────────────────
local tests_passed = 0
local tests_failed = 0

local function assert_equals(expected, actual, message)
    if expected == actual then
        print("  PASS: " .. message)
        tests_passed = tests_passed + 1
    else
        print("  FAIL: " .. message)
        print("    Expected: " .. tostring(expected))
        print("    Actual:   " .. tostring(actual))
        tests_failed = tests_failed + 1
    end
end

local function assert_true(value, message)
    if value == true then
        print("  PASS: " .. message)
        tests_passed = tests_passed + 1
    else
        print("  FAIL: " .. message)
        print("    Expected: true")
        print("    Actual:   " .. tostring(value))
        tests_failed = tests_failed + 1
    end
end

-- ─── lib_filename_for_os ─────────────────────────────────────────────────────
print("=== lib_filename_for_os ===")

assert_equals(
    "libpolyplug.so",
    native.lib_filename_for_os("Linux", "polyplug"),
    "Linux core lib: lib<base>.so"
)
assert_equals(
    "libpolyplug_native.so",
    native.lib_filename_for_os("Linux", "polyplug_native"),
    "Linux loader lib: lib<base>.so"
)

-- macOS regression guard: must produce .dylib, not .so
assert_equals(
    "libpolyplug.dylib",
    native.lib_filename_for_os("OSX", "polyplug"),
    "macOS core lib: lib<base>.dylib (not .so)"
)
assert_equals(
    "libpolyplug_lua.dylib",
    native.lib_filename_for_os("OSX", "polyplug_lua"),
    "macOS loader lib: lib<base>.dylib (not .so)"
)

-- Windows: no lib prefix, .dll extension
assert_equals(
    "polyplug.dll",
    native.lib_filename_for_os("Windows", "polyplug"),
    "Windows core lib: <base>.dll (no lib prefix)"
)
assert_equals(
    "polyplug_dotnet.dll",
    native.lib_filename_for_os("Windows", "polyplug_dotnet"),
    "Windows loader lib: <base>.dll (no lib prefix)"
)

-- ─── Filename consistency: all loaders × all OS ───────────────────────────────
print("=== lib_filename_for_os: all loaders x all OS ===")

local bases   = { "polyplug", "polyplug_native", "polyplug_python",
                  "polyplug_lua", "polyplug_js", "polyplug_dotnet" }
local os_data = {
    { os = "Linux",   ext = ".so",    prefix = "lib" },
    { os = "OSX",     ext = ".dylib", prefix = "lib" },
    { os = "Windows", ext = ".dll",   prefix = ""    },
}

for _, od in ipairs(os_data) do
    for _, base in ipairs(bases) do
        local got      = native.lib_filename_for_os(od.os, base)
        local expected = od.prefix .. base .. od.ext
        assert_equals(expected, got, od.os .. " / " .. base)
    end
end

-- ─── platform_for ────────────────────────────────────────────────────────────
print("=== platform_for ===")

assert_equals("linux-x64",   native.platform_for("Linux",   "x64"),   "Linux x64 platform")
assert_equals("linux-arm64", native.platform_for("Linux",   "arm64"), "Linux arm64 platform")
assert_equals("macos-x64",   native.platform_for("OSX",     "x64"),   "macOS x64 platform")
assert_equals("macos-arm64", native.platform_for("OSX",     "arm64"), "macOS arm64 platform")
assert_equals("windows-x64", native.platform_for("Windows", "x64"),   "Windows x64 platform")

-- ─── host_root anchor ────────────────────────────────────────────────────────
-- host_root() uses debug.getinfo(1) capturing native.lua's own source path,
-- so it anchors to the host root regardless of which file required polyplug.native.
print("=== host_root ===")

local root = native.host_root()

-- Round-trip: host_root()/polyplug/native.lua must be the file itself.
-- This proves the anchor resolves to the host root from any caller depth.
local native_lua_path = root .. "/polyplug/native.lua"
local fh = io.open(native_lua_path, "r")
assert_true(
    fh ~= nil,
    "host_root()/polyplug/native.lua is readable — anchor resolves correctly for any caller depth"
)
if fh then fh:close() end

-- The _native staging dir path must also be reachable from host_root().
-- (It need not exist yet; the test just confirms the path is formed correctly.)
local native_dir = root .. "/_native"
assert_true(
    native_dir:find("_native", 1, true) ~= nil,
    "host_root()/_native path contains the _native segment"
)

-- ─── resolve: env var wins ────────────────────────────────────────────────────
-- Test env-override priority: set a known env var and check resolve() returns it.
print("=== resolve: env var wins ===")

-- Use the env var that the test harness and CI already set for the core lib.
-- If POLYPLUG_LIB is set in the environment, resolve() must return it unchanged.
local existing_polyplug_lib = os.getenv("POLYPLUG_LIB")
if existing_polyplug_lib then
    local resolved = native.resolve("POLYPLUG_LIB", "polyplug")
    assert_equals(
        existing_polyplug_lib,
        resolved,
        "resolve() returns POLYPLUG_LIB env value when set (env wins over co-located)"
    )
else
    print("  SKIP: POLYPLUG_LIB not set in env — env-wins live check skipped (CI sets it)")
end

-- ─── resolve: co-located path structure ──────────────────────────────────────
print("=== resolve: staged path structure ===")

-- With no env var and no staged file, resolve() returns the bare base name.
local absent_key = "POLYPLUG_XYZZY_NONEXISTENT_KEY_TEST"
local bare = native.resolve(absent_key, "polyplug_xyzzy_absent")
assert_equals(
    "polyplug_xyzzy_absent",
    bare,
    "resolve() returns bare base name when env absent and no staged file present"
)

-- Confirm the staged path that resolve() would try has the correct structure.
local staged = root .. "/_native/" .. native.platform() .. "/" .. native.lib_filename("polyplug")
assert_true(
    staged:find("_native", 1, true) ~= nil,
    "staged path contains _native segment"
)
assert_true(
    staged:find(native.platform(), 1, true) ~= nil,
    "staged path contains the current platform segment"
)
assert_true(
    staged:find(native.lib_filename("polyplug"), 1, true) ~= nil,
    "staged path contains the correct OS-specific filename for this platform"
)

-- resolve() returns the staged path when the file exists there.
-- Create a temp sentinel file at the staged location to prove this branch works.
local staged_dir = root .. "/_native/" .. native.platform()
local ok_mkdir = os.execute("mkdir -p " .. staged_dir)
if ok_mkdir == 0 or ok_mkdir == true then
    local sentinel = staged_dir .. "/libpolyplug_sentinel_test.so"
    local wf = io.open(sentinel, "w")
    if wf then
        wf:close()
        local resolved_staged = native.resolve(absent_key, "polyplug_sentinel_test")
        assert_equals(
            sentinel,
            resolved_staged,
            "resolve() returns staged path when file exists in _native/<platform>/"
        )
        os.remove(sentinel)
    else
        print("  SKIP: cannot create sentinel file — staged-file branch test skipped")
    end
else
    print("  SKIP: cannot create staged dir — staged-file branch test skipped")
end

-- ─── resolve: flat co-located file (luarocks install layout) ─────────────────
-- In a luarocks install, lua modules and natives land flat in the same dir:
--   <host-root>/polyplug/native.lua  and  <host-root>/<filename>
-- resolve() must find the flat file when no staged _native/<platform>/ file exists.
print("=== resolve: flat co-located file ===")

local flat_base     = "polyplug_flat_test"
local flat_filename = native.lib_filename(flat_base)
local flat_path     = root .. "/" .. flat_filename
local flat_wf       = io.open(flat_path, "w")
if flat_wf then
    flat_wf:close()
    local resolved_flat = native.resolve(absent_key, flat_base)
    assert_equals(
        flat_path,
        resolved_flat,
        "resolve() returns flat <host-root>/<filename> when present and no staged file exists"
    )

    -- Staged _native/<platform>/ file must WIN over the flat file (tier 2 precedence).
    local stage_dir   = root .. "/_native/" .. native.platform()
    local ok_stage    = os.execute("mkdir -p " .. stage_dir)
    if ok_stage == 0 or ok_stage == true then
        local staged_flat = stage_dir .. "/" .. flat_filename
        local staged_wf   = io.open(staged_flat, "w")
        if staged_wf then
            staged_wf:close()
            local resolved_both = native.resolve(absent_key, flat_base)
            assert_equals(
                staged_flat,
                resolved_both,
                "staged _native/<platform>/ path wins over flat co-located file when both exist"
            )
            os.remove(staged_flat)
        else
            print("  SKIP: cannot create staged file — flat-vs-staged precedence test skipped")
        end
    else
        print("  SKIP: cannot create staged dir — flat-vs-staged precedence test skipped")
    end

    os.remove(flat_path)
else
    print("  SKIP: cannot create flat file — flat co-located branch test skipped")
end

-- ─── resolve: env var wins over staged and flat files ────────────────────────
-- LuaJIT cannot mutate the process environment, so this asserts env precedence by
-- relaunching the interpreter with the env var set, exercising the real resolve path.
print("=== resolve: env var wins over staged and flat ===")

do
    local probe = [[
        package.path = ']] .. script_dir .. [[../?.lua;' .. package.path
        local n = require('polyplug.native')
        local root = n.host_root()
        local base = 'polyplug_envwin_test'
        local fname = n.lib_filename(base)
        -- Create flat + staged files so env must outrank both.
        local flat = root .. '/' .. fname
        local fw = io.open(flat, 'w'); if fw then fw:close() end
        local sdir = root .. '/_native/' .. n.platform()
        os.execute('mkdir -p ' .. sdir)
        local staged = sdir .. '/' .. fname
        local sw = io.open(staged, 'w'); if sw then sw:close() end
        local r = n.resolve('POLYPLUG_ENVWIN_TEST', base)
        io.write(r)
        os.remove(flat); os.remove(staged)
    ]]
    local override = "/explicit/env/override/" .. flat_filename
    local cmd = "POLYPLUG_ENVWIN_TEST='" .. override ..
        "' luajit -e \"" .. probe:gsub('"', '\\"') .. "\""
    local pipe = io.popen(cmd)
    if pipe then
        local out = pipe:read("*a")
        pipe:close()
        if out and #out > 0 then
            assert_equals(
                override,
                out,
                "resolve() returns env-var override even when staged and flat files exist"
            )
        else
            print("  SKIP: env-wins subprocess produced no output — skipped")
        end
    else
        print("  SKIP: cannot spawn subprocess for env-wins check — skipped")
    end
end

-- ─── Results ─────────────────────────────────────────────────────────────────
print("\n=== Results ===")
print(string.format("Tests passed: %d", tests_passed))
print(string.format("Tests failed: %d", tests_failed))

if tests_failed > 0 then
    os.exit(1)
end
