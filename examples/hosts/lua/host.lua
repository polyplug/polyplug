-- Pipeline Host — Lua host demonstrating polyplug usage.
--
-- Creates a runtime, registers the language loaders, scans the plugin directory,
-- loads every bundle it can, prints the contracts each loaded bundle provides
-- (discovery, mirroring the C++ reference host), then resolves the pipeline
-- contracts through the GENERATED callers and dispatches the full
-- decode -> transform -> encode -> report -> validate pipeline (mirroring the
-- rust/python/csharp reference hosts).

local polyplug = require('polyplug')
local abi = require('polyplug_abi')
-- Generated guest-contract callers: resolve + dispatch via the host ABI path.
-- Required AFTER polyplug/polyplug_abi so the ffi.cdef ABI types the callers
-- reference (GuestContractInterface, AbiError, StringView, ...) are defined.
local callers = require('generated.host.callers')
-- Generated host-contract interface factories (host.logger). Also required
-- AFTER polyplug_abi for the same cdef ordering reason.
local interface_factories = require('generated.host.interface_factories')

-- Allow overriding the core library via POLYPLUG_LIB (verify_hosts.sh sets it).
local lib_override = os.getenv('POLYPLUG_LIB')
if lib_override then
    polyplug.load_lib(lib_override)
end

local native_loader = require('polyplug.loaders.native')
local lua_loader = require('polyplug.loaders.lua')

--- Resolve the plugin directory from the environment or a few fallbacks.
-- @return string Path to the directory containing plugin bundles.
local function get_plugin_path()
    local path = os.getenv('POLYPLUG_PLUGIN_PATH')
    if path then
        return path
    end

    local candidates = {
        'examples/plugins',
        '../../../examples/plugins',
        '../../examples/plugins',
    }
    for _, candidate in ipairs(candidates) do
        local f = io.open(candidate .. '/rust_decoder/manifest.toml', 'r')
        if f then
            f:close()
            return candidate
        end
    end
    return 'examples/plugins'
end

--- List the bundle directories under the plugin path (sorted).
-- @param plugin_path string Directory to scan.
-- @return table Array of absolute bundle directory paths.
local function list_bundle_dirs(plugin_path)
    local dirs = {}
    local pipe = io.popen('ls -d "' .. plugin_path .. '"/*/ 2>/dev/null')
    if not pipe then
        return dirs
    end
    for line in pipe:lines() do
        dirs[#dirs + 1] = (line:gsub('/$', ''))
    end
    pipe:close()
    return dirs
end

--- Read a manifest's name and provides list.
-- @param manifest_path string Path to manifest.toml.
-- @return string|nil, table Bundle name and array of provided contract strings.
local function parse_manifest(manifest_path)
    local f = io.open(manifest_path, 'r')
    if not f then
        return nil, {}
    end
    local content = f:read('*a')
    f:close()

    local name = content:match('name%s*=%s*"([^"]+)"')
    local provides = {}
    local list = content:match('provides%s*=%s*%[([^%]]*)%]')
    if list then
        for item in list:gmatch('"([^"]+)"') do
            provides[#provides + 1] = item
        end
    end
    return name, provides
end

local plugin_path = get_plugin_path()
io.stderr:write('loading plugins from: ' .. plugin_path .. '\n\n')

-- Custom runtime logger: receives every runtime diagnostic AND guest log
-- lines routed through the funnel (HostApi.log -> RuntimeConfig.log). The SDK
-- routes it through the polyplug_lua loader cdylib's log trampoline because
-- LuaJIT callbacks cannot receive the ABI's by-value StringViews. Output goes
-- to STDERR only — verify_hosts.sh requires the hosts' pipeline stdout to stay
-- byte-identical across languages. The callback runs on whatever thread the
-- runtime logs from.
local rt = polyplug.Runtime.new({
    log = function(level, scope, message)
        io.stderr:write(string.format('[host-log][%d][%s] %s\n', level, scope, message))
    end,
    -- Info: also surface guest lifecycle log lines, not just Error/Warn.
    log_max_level = polyplug.LogLevel.Info,
})

-- Register the loaders exercised by this host pipeline. An unavailable backing
-- cdylib is skipped so the remaining providers can still run.
local loaders = {
    { name = 'native', register = function() native_loader.register(rt) end },
    { name = 'lua', register = function() lua_loader.register(rt) end },
}
for _, loader in ipairs(loaders) do
    local ok, err = pcall(loader.register)
    if not ok then
        io.stderr:write('  loader ' .. loader.name .. ' unavailable: ' .. tostring(err) .. '\n')
    end
end

-- Host-side implementation of the `host.logger` contract, registered through
-- the GENERATED interface factory so plugins can call back into the host
-- (mirrors the rust/cpp reference hosts' ConsoleLogger).
local ConsoleLogger = {}
ConsoleLogger.__index = ConsoleLogger

-- Factory: the generated interface calls this once per instance (or once total
-- for a singleton contract), so each instance gets its own fresh impl table.
function ConsoleLogger.new()
    return setmetatable({}, ConsoleLogger)
end

function ConsoleLogger:log(message)
    print('[plugin] ' .. message)
end

function ConsoleLogger:log_with_level(level, message)
    local level_names = { [0] = 'DEBUG', [1] = 'INFO', [2] = 'WARN', [3] = 'ERROR' }
    local level_str = level_names[tonumber(level)] or 'INFO'
    print(string.format('[plugin][%s] %s', level_str, message))
end

local logger_iface = interface_factories.create_host_logger_interface(
    ConsoleLogger.new, lua_loader.bridge_lib())
rt:register_host_contract(logger_iface)

local bundle_dirs = list_bundle_dirs(plugin_path)
if #bundle_dirs == 0 then
    io.stderr:write('no plugins found in ' .. plugin_path .. '\n')
    os.exit(1)
end

io.stderr:write('discovered ' .. #bundle_dirs .. ' bundles\n\n')

local loaded = {}
for _, dir in ipairs(bundle_dirs) do
    local name, provides = parse_manifest(dir .. '/manifest.toml')
    if name and name:match('^cpp_') then
        local ok, err = pcall(function() rt:load_bundle(dir) end)
        if ok then
            loaded[#loaded + 1] = { name = name, provides = provides }
            io.stderr:write('  loaded: ' .. name .. '\n')
        else
            io.stderr:write('  skipped ' .. name .. ': ' .. tostring(err):gsub('\n.*', '') .. '\n')
        end
    end
end

if #loaded == 0 then
    io.stderr:write('no bundles could be loaded\n')
    os.exit(1)
end

print('\n=== Pipeline Host (Lua) ===\n')

-- Discovery output (kept for parity with the other reference hosts).
for _, bundle in ipairs(loaded) do
    local bid = polyplug.bundle_id(bundle.name)
    for _, contract in ipairs(bundle.provides) do
        local contract_name, major = contract:match('([^@]+)@(%d+)')
        if contract_name then
            local cid = polyplug.guest_contract_id(contract_name, tonumber(major))
            print(string.format('[%s] provides %s (bundle_id=0x%016x, contract_id=0x%016x)',
                bundle.name, contract, bid, cid))
        end
    end
end

-- Full dispatch pipeline: resolve each contract through the generated callers
-- and invoke it. Mirrors hosts/python/host.py and hosts/rust/src/main.rs.
print('')
local input_str = 'name,value,42'
print('Input: "' .. input_str .. '"\n')

local host = rt:host()

local decoder = callers.PipelineDecoderContract_create(rt, host)
if decoder then
    local result = abi.to_str(decoder:decode(input_str))
    print(string.format('[decoder] decode("%s") = "%s"', input_str, result))
end

local decoded = 'DECODED:' .. input_str:gsub(',', '|')
local transformer = callers.DataTransformerContract_create(rt, host)
if transformer then
    local result = abi.to_str(transformer:transform(decoded))
    print(string.format('[transformer] transform("%s") = "%s"', decoded, result))
end

local transformed = 'TRANSFORMED:NAME|value (transformed)|43'
local encoder = callers.PipelineEncoderContract_create(rt, host)
if encoder then
    local result = abi.to_str(encoder:encode(transformed))
    print(string.format('[encoder] encode("%s") = "%s"', transformed, result))
end

local reporter = callers.DataReporterContract_create(rt, host)
if reporter then
    local result = abi.to_str(reporter:report(transformed))
    print(string.format('[reporter] report("%s") = "%s"', transformed, result))
end

local validator = callers.PipelineValidatorContract_create(rt, host)
if validator then
    local result = abi.to_str(validator:validate(decoded))
    print(string.format('[validator] validate("%s") = "%s"', decoded, result))
end

-- Round-trip micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS): times the full
-- host → runtime → native guest → return path (Lua host calling the native decoder
-- plugin and getting a string back). Point POLYPLUG_PLUGIN_PATH at native guests only.
local bench_iters = os.getenv('POLYPLUG_BENCH_ITERS')
if bench_iters then
    local n = tonumber(bench_iters)
    local bench_decoder = callers.PipelineDecoderContract_create(rt, host)
    if bench_decoder then
        local warmup = math.min(n, 10000)
        for _ = 1, warmup do bench_decoder:decode(input_str) end
        local t0 = os.clock()
        for _ = 1, n do bench_decoder:decode(input_str) end
        local t1 = os.clock()
        print(string.format('ROUNDTRIP_NS=%.2f LANG=lua', (t1 - t0) * 1e9 / n))
    end
end

-- Host-call micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS): times the BARE
-- host → runtime call — one find_guest_contract per iteration through the HostApi
-- function pointer (one LuaJIT FFI hop + the runtime's registry lookup), no guest
-- dispatch. Every returned handle is null-checked and the hit count is verified,
-- so the lookup result is observably consumed each iteration.
if bench_iters then
    local n = tonumber(bench_iters)
    if n and n > 0 then
        local cid = callers.PIPELINE_DECODER_CONTRACT_ID
        local warmup = math.min(n, 10000)
        local hits = 0
        for _ = 1, warmup do
            if rt:find_guest_contract(cid, 0).index ~= polyplug.NULL_HANDLE_INDEX then
                hits = hits + 1
            end
        end
        hits = 0
        local t0 = os.clock()
        for _ = 1, n do
            if rt:find_guest_contract(cid, 0).index ~= polyplug.NULL_HANDLE_INDEX then
                hits = hits + 1
            end
        end
        local t1 = os.clock()
        if hits == n then
            print(string.format('HOSTCALL_NS=%.2f LANG=lua', (t1 - t0) * 1e9 / n))
        else
            io.stderr:write(string.format(
                'HOSTCALL bench: lookup missed (%d/%d hits) — no result printed\n', hits, n))
        end
    end
end

print('\ndone.')
