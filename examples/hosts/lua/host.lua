-- Pipeline Host — Lua host demonstrating polyplug usage

local polyplug = require('polyplug')
polyplug.load_lib('libpolyplug.so')

local function get_plugin_path()
    return os.getenv('POLYPLUG_PLUGIN_PATH') or 'examples/plugins'
end

local plugin_path = get_plugin_path()
print('loading plugins from: ' .. plugin_path .. '\n')

local rt = polyplug.Runtime.new()
polyplug.register_native_loader(rt._ptr)

local bundles = polyplug.scanner.scan_dir(plugin_path)
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

for _, bundle in ipairs(bundles) do
    local manifest = bundle.manifest
    for _, contract in ipairs(manifest.provides or {}) do
        if contract:match('^pipeline.Decoder') then
            local handle = rt:find_by_bundle(manifest.bundle_name, 'pipeline.Decoder', 1)
            if handle then
                print(string.format('[%s] decoder ready', manifest.bundle_name))
            end
        end
    end
end

print('\ndone.')
