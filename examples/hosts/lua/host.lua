-- Pipeline Host — Lua host demonstrating polyplug usage

local ffi = require('ffi')
local polyplug = require('polyplug')
polyplug.load_lib(os.getenv('POLYPLUG_LIB_PATH') or 'libpolyplug.so')

local runtime_mod = require('polyplug.runtime')

local PIPELINE_DECODER_CONTRACT_ID = 0x12F3C106B0C3DC1EULL
local DATA_TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFULL
local PIPELINE_ENCODER_CONTRACT_ID = 0x127D1703C6EFB432ULL
local DATA_REPORTER_CONTRACT_ID = 0x81D41D43E511D297ULL
local PIPELINE_VALIDATOR_CONTRACT_ID = 0xA553FAB5D11C7AF0ULL

local function get_plugin_path()
    local path = os.getenv('POLYPLUG_PLUGIN_PATH')
    if path then return path end

    local candidates = {
        'examples/plugins',
        '../../../examples/plugins',
        '../../examples/plugins',
        '/mnt/data/Projects/Utils/polyplug/examples/plugins'
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

local plugin_path = get_plugin_path()
print('loading plugins from: ' .. plugin_path .. '\n')

local _instances = {}

local config = {
    hot_reload_max_retries = 5,
    hot_reload_retry_interval_ms = 200,
    hot_reload_abort_on_max_retries = false
}
runtime_mod.set_config(config)

runtime_mod.on_reload(function(phase)
    local reload_phase = require('polyplug.reload_phase')
    if reload_phase.is_preparing(phase) then
        print(string.format('[HOT-RELOAD] Preparing: %s (bundle_id=0x%016X, retry %d)',
            phase.bundle_name, phase.bundle_id, phase.retry_count))
        if _instances[phase.bundle_id] then
            _instances[phase.bundle_id] = nil
            print('[HOT-RELOAD] Cleared instances for bundle ' .. phase.bundle_name)
        end
    elseif reload_phase.is_reloaded(phase) then
        print(string.format('[HOT-RELOAD] Reloaded: %s (bundle_id=0x%016X)',
            phase.bundle_name, phase.bundle_id))
    elseif reload_phase.is_failed(phase) then
        print(string.format('[HOT-RELOAD] Failed: %s (bundle_id=0x%016X) - %s',
            phase.bundle_name, phase.bundle_id, phase.reason))
    end
end)

local rt = polyplug.Runtime.new()

local ok, err = pcall(function()
    polyplug.register_native_loader(rt._ptr)
end)
if not ok and not string.find(err, 'failed: 2') then
    error(err)
end

local bundles = polyplug.scan_dir(plugin_path)
if #bundles == 0 then
    print('no plugins found in ' .. plugin_path)
    os.exit(1)
end

print('discovered ' .. #bundles .. ' bundles\n')

for _, bundle in ipairs(bundles) do
    rt:load_bundle(tostring(bundle.path))
    local name = bundle.manifest.name or 'unknown'
    print('  loaded: ' .. name)
end

print('\n=== Pipeline Host (Lua) ===\n')

local input_str = 'name,value,42'
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

local result = call_contract(rt, PIPELINE_DECODER_CONTRACT_ID, input_str)
if result then
    print('[decoder] decode("' .. input_str .. '") = "' .. result .. '"')
end

local decoded = 'DECODED:' .. input_str:gsub(',', '|')
local result = call_contract(rt, DATA_TRANSFORMER_CONTRACT_ID, decoded)
if result then
    print('[transformer] transform("' .. decoded .. '") = "' .. result .. '"')
end

local transformed = 'TRANSFORMED:NAME|value (transformed)|43'
local result = call_contract(rt, PIPELINE_ENCODER_CONTRACT_ID, transformed)
if result then
    print('[encoder] encode("' .. transformed .. '") = "' .. result .. '"')
end

local result = call_contract(rt, DATA_REPORTER_CONTRACT_ID, transformed)
if result then
    print('[reporter] report("' .. transformed .. '") = "' .. result .. '"')
end

local result = call_contract(rt, PIPELINE_VALIDATOR_CONTRACT_ID, decoded)
if result then
    print('[validator] validate("' .. decoded .. '") = "' .. result .. '"')
end

print('\ndone.')