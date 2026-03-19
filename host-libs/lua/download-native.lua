#!/usr/bin/env luajit

local http = require("socket.http")
local ltn12 = require("ltn12")
local ffi = require("ffi")

local GITHUB_REPO = "polyplug/polyplug"
local VERSION = arg[1] or "v1.0.0"

local PLATFORM_CONFIG = {
    {
        platform = "linux-x64",
        remote_name = "libpolyplug-linux-x64.so",
        local_name = "libpolyplug.so",
        dir = "_native/linux-x64"
    },
    {
        platform = "darwin-x64",
        remote_name = "libpolyplug-macos-x64.dylib",
        local_name = "libpolyplug.dylib",
        dir = "_native/darwin-x64"
    },
    {
        platform = "darwin-arm64",
        remote_name = "libpolyplug-macos-arm64.dylib",
        local_name = "libpolyplug.dylib",
        dir = "_native/darwin-arm64"
    },
    {
        platform = "win32-x64",
        remote_name = "polyplug-windows-x64.dll",
        local_name = "polyplug.dll",
        dir = "_native/win32-x64"
    }
}

local function download_file(url, output_path)
    local file = io.open(output_path, "wb")
    if not file then
        return false, "Failed to open file: " .. output_path
    end
    
    local result, code, headers, status = http.request{
        url = url,
        sink = ltn12.sink.file(file),
        redirect = true
    }
    
    if not result then
        return false, "Download failed: " .. tostring(code)
    end
    
    if code ~= 200 then
        return false, "HTTP error: " .. tostring(code) .. " " .. tostring(status)
    end
    
    return true
end

local function ensure_dir(dir_path)
    os.execute("mkdir -p " .. dir_path)
end

local function main()
    print("Downloading native libraries for " .. GITHUB_REPO .. " @" .. VERSION)
    print("")
    
    local base_url = string.format(
        "https://github.com/%s/releases/download/%s/%s",
        GITHUB_REPO,
        VERSION,
        "%s"
    )
    
    local success_count = 0
    local fail_count = 0
    
    for _, config in ipairs(PLATFORM_CONFIG) do
        local url = string.format(base_url, config.remote_name)
        local output_path = config.dir .. "/" .. config.local_name
        
        print("Downloading: " .. config.platform)
        print("  URL: " .. url)
        print("  Output: " .. output_path)
        
        ensure_dir(config.dir)
        
        local ok, err = download_file(url, output_path)
        if ok then
            print("  Status: OK")
            success_count = success_count + 1
        else
            print("  Status: FAILED - " .. err)
            fail_count = fail_count + 1
        end
        print("")
    end
    
    print("Summary:")
    print("  Success: " .. success_count)
    print("  Failed: " .. fail_count)
    
    if fail_count > 0 then
        os.exit(1)
    end
end

main()
