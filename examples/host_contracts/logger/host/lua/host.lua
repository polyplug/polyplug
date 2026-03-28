-- Logger Host — Lua host demonstrating host contracts

local ffi = require('ffi')
local polyplug = require('polyplug')

local lib_path = os.getenv('POLYPLUG_LIB')
if lib_path then
    polyplug.load_lib(lib_path)
end

local callers = require('generated.host.callers')
local contracts = require('generated.host.contracts')

local function get_plugin_path()
    local path = os.getenv('POLYPLUG_PLUGIN_PATH')
    if path then return path end

    local candidates = {
        'examples/host_contracts/logger/plugins',
        '../../../plugins',
        '../../plugins',
        '/mnt/data/Projects/Utils/polyplug/examples/host_contracts/logger/plugins'
    }

    for _, candidate in ipairs(candidates) do
        local f = io.open(candidate .. '/rust_worker/manifest.toml', 'r')
        if f then
            f:close()
            return candidate
        end
    end

    return 'examples/host_contracts/logger/plugins'
end

local plugin_path = get_plugin_path()
print('loading plugins from: ' .. plugin_path .. '\n')

local rt = polyplug.Runtime.new()

local ok, err = pcall(function()
    polyplug.register_native_loader(rt._ptr)
end)
if not ok and not string.find(err, 'failed: 2') then
    error(err)
end

local function log_impl(message)
    print('[PLUGIN LOG] ' .. message)
end

rt:register_host_contract(contracts.HOSTLOGGER_CONTRACT_ID, log_impl)

local bundles = {}
for name in io.popen('ls -1 "' .. plugin_path .. '" 2>/dev/null'):lines() do
    local manifest_path = plugin_path .. '/' .. name .. '/manifest.toml'
    local f = io.open(manifest_path, 'r')
    if f then
        f:close()
        rt:load_bundle(plugin_path .. '/' .. name)
        bundles[#bundles + 1] = name
        print('  loaded: ' .. name)
    end
end

if #bundles == 0 then
    print('no plugins found in ' .. plugin_path)
    os.exit(1)
end

print('\ndiscovered ' .. #bundles .. ' bundles\n')

print('\n=== Logger Host (Lua) ===\n')

local input_str = 'hello world'
print('Input: "' .. input_str .. '"\n')

local function call_contract(rt, contract_id, input)
    local handle = rt:find_by_contract(contract_id, 0)
    if handle == polyplug.NULL_HANDLE then
        return nil
    end
    local guard = rt:resolve_plugin(handle)
    if not guard then
        return nil
    end
    return guard:call(0, input)
end

local result = call_contract(rt, callers.EXAMPLE_WORKER_CONTRACT_ID, input_str)
if result then
    print('[host] do_work("' .. input_str .. '") = "' .. result .. '"')
end

print('\ndone.')