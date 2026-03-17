-- Pipeline Host — Lua host demonstrating polyplug usage

local polyplug = require('polyplug')
polyplug.load_lib(os.getenv('POLYPLUG_LIB_PATH') or 'libpolyplug.so')

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
    local name = bundle.manifest.bundle_name or 'unknown'
    print('  loaded: ' .. name)
end

print('\n=== Pipeline Host (Lua) ===\n')

local input_str = 'name,value,42'
print('Input: "' .. input_str .. '"\n')

for _, bundle in ipairs(bundles) do
    local bundle_name = bundle.manifest.bundle_name or 'unknown'
    local bid = polyplug.bundle_id(bundle_name)
    local provides = bundle.manifest.provides or {}
    
    for _, contract in ipairs(provides) do
        local contract_name, version_str = contract:match('([^@]+)@(.+)')
        if contract_name then
            local major = tonumber(version_str:match('^(%d+)')) or 1
            local cid = polyplug.contract_id(contract_name, major)
            local handle = rt:find_by_bundle(bid, cid, 0)
            
            if handle ~= polyplug.NULL_HANDLE then
                if contract_name == 'pipeline.Decoder' then
                    local result = polyplug.call_plugin_fn(rt._ptr, handle, 0, input_str)
                    print('[' .. bundle_name .. '] decode("' .. input_str .. '") = "' .. result .. '"')
                elseif contract_name == 'data.Transformer' then
                    local decoded = 'DECODED:' .. input_str:gsub(',', '|')
                    local result = polyplug.call_plugin_fn(rt._ptr, handle, 0, decoded)
                    print('[' .. bundle_name .. '] transform("' .. decoded .. '") = "' .. result .. '"')
                elseif contract_name == 'pipeline.Encoder' then
                    local transformed = 'TRANSFORMED:NAME|value (transformed)|43'
                    local result = polyplug.call_plugin_fn(rt._ptr, handle, 0, transformed)
                    print('[' .. bundle_name .. '] encode("' .. transformed .. '") = "' .. result .. '"')
                elseif contract_name == 'data.Reporter' then
                    local transformed = 'TRANSFORMED:NAME|value (transformed)|43'
                    local result = polyplug.call_plugin_fn(rt._ptr, handle, 0, transformed)
                    print('[' .. bundle_name .. '] report("' .. transformed .. '") = "' .. result .. '"')
                elseif contract_name == 'pipeline.Validator' then
                    local decoded = 'DECODED:' .. input_str:gsub(',', '|')
                    local result = polyplug.call_plugin_fn(rt._ptr, handle, 0, decoded)
                    print('[' .. bundle_name .. '] validate("' .. decoded .. '") = "' .. result .. '"')
                end
            end
        end
    end
end

print('\ndone.')
