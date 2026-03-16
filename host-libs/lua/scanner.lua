-- Scanner module for discovering polyplug plugin bundles

local ffi = require('ffi')
local M = {}

local function parse_toml(content)
    local result = {}
    local current_section = nil
    
    for line in content:gmatch('[^\n]+') do
        line = line:gsub('^%s+', ''):gsub('%s+$', '')
        if #line > 0 and not line:match('^#') then
            -- Section header
            local section = line:match('^%[(.+)%]$')
            if section then
                current_section = section
                result[current_section] = {}
            else
                -- Key-value pair
                local key, value = line:match('^(%w+)%s*=%s*(.+)$')
                if key and value then
                    value = value:gsub('^"', ''):gsub('"$', '')
                    if value:match('^%[') then
                        -- Array
                        local arr = {}
                        for item in value:gmatch('"([^"]+)"') do
                            table.insert(arr, item)
                        end
                        value = arr
                    elseif value:match('^%d+$') then
                        value = tonumber(value)
                    end
                    
                    if current_section then
                        result[current_section][key] = value
                    else
                        result[key] = value
                    end
                end
            end
        end
    end
    
    return result
end

function M.scan_dir(dir_path)
    local bundles = {}
    
    local dir = io.popen('find "' .. dir_path .. '" -maxdepth 1 -type d -mindepth 1')
    for subdir in dir:lines() do
        local manifest_path = subdir .. '/manifest.toml'
        local file = io.open(manifest_path, 'r')
        if file then
            local content = file:read('*all')
            file:close()
            local manifest = parse_toml(content)
            table.insert(bundles, { path = subdir, manifest = manifest })
        end
    end
    dir:close()
    
    return bundles
end

return M
