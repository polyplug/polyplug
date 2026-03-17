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
print('Lua host loaded all plugins successfully!')
print('\ndone.')
