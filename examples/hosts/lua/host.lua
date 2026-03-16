-- Pipeline Host — Lua host demonstrating polyplug usage

local polyplug = require('polyplug')
polyplug.load_lib(os.getenv('POLYPLUG_LIB_PATH') or 'libpolyplug.so')

local function get_plugin_path()
    return os.getenv('POLYPLUG_PLUGIN_PATH') or 'examples/plugins'
end

local plugin_path = get_plugin_path()
print('loading plugins from: ' .. plugin_path .. '\n')

local rt = polyplug.Runtime.new()

-- Register native loader (ignore error code 2 = duplicate)
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
    print('  loaded: ' .. bundle.manifest.bundle_name)
end

print('\n=== Pipeline Host (Lua) ===\n')

local input = 'name,value,42'
print('Input: "' .. input .. '"\n')

for _, bundle in ipairs(bundles) do
    local manifest = bundle.manifest
    local provides = manifest.provides or {}
    local bundle_name = manifest.bundle_name
    
    for _, contract in ipairs(provides) do
        if contract:match('^pipeline.Decoder@1') then
            local handle = rt:find_by_bundle(bundle_name, 'pipeline.Decoder', 1)
            if handle then
                local result = rt:call(handle, 'decode', input)
                print(string.format('[%s] decode("%s") = "%s"', bundle_name, input, result))
            end
        end
        
        if contract:match('^data.Transformer@1') then
            local handle = rt:find_by_bundle(bundle_name, 'data.Transformer', 1)
            if handle then
                local decoded = 'DECODED:' .. input:gsub(',', '|')
                local result = rt:call(handle, 'transform', decoded)
                print(string.format('[%s] transform("%s") = "%s"', bundle_name, decoded, result))
            end
        end
        
        if contract:match('^pipeline.Encoder@1') then
            local handle = rt:find_by_bundle(bundle_name, 'pipeline.Encoder', 1)
            if handle then
                local transformed = 'TRANSFORMED:NAME|value (transformed)|43'
                local result = rt:call(handle, 'encode', transformed)
                print(string.format('[%s] encode("%s") = "%s"', bundle_name, transformed, result))
            end
        end
        
        if contract:match('^data.Reporter@1') then
            local handle = rt:find_by_bundle(bundle_name, 'data.Reporter', 1)
            if handle then
                local transformed = 'TRANSFORMED:NAME|value (transformed)|43'
                local result = rt:call(handle, 'report', transformed)
                print(string.format('[%s] report("%s") = "%s"', bundle_name, transformed, result))
            end
        end
        
        if contract:match('^pipeline.Validator@1') then
            local handle = rt:find_by_bundle(bundle_name, 'pipeline.Validator', 1)
            if handle then
                local decoded = 'DECODED:' .. input:gsub(',', '|')
                local result = rt:call(handle, 'validate', decoded)
                print(string.format('[%s] validate("%s") = "%s"', bundle_name, decoded, result))
            end
        end
    end
end

print('\ndone.')
