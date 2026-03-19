# Cross-Platform Native Library Embedding Plan

## Goal

**"Just add polyplug as a dependency, and it works on Linux, macOS, and Windows."**

No manual downloads. No "build from source" requirements. No obstacles.

---

## Architecture

### Native Library Matrix

| Platform | Architecture | Rust Target | Output File |
|----------|-------------|-------------|-------------|
| Linux | x86_64 | x86_64-unknown-linux-gnu | libpolyplug.so |
| macOS | x86_64 | x86_64-apple-darwin | libpolyplug.dylib |
| macOS | aarch64 | aarch64-apple-darwin | libpolyplug.dylib |
| Windows | x86_64 | x86_64-pc-windows-msvc | polyplug.dll |

### Package Embedding Strategy

#### NuGet (C#)
```
Polyplug.nupkg/
├── lib/net10.0/Polyplug.dll
└── runtimes/
    ├── linux-x64/native/libpolyplug.so
    ├── osx-x64/native/libpolyplug.dylib
    ├── osx-arm64/native/libpolyplug.dylib
    └── win-x64/native/polyplug.dll
```
- .NET auto-loads via `[DllImport("polyplug")]` + RID detection

#### Python (PyPI)
```
polyplug-0.1.0-py3-none-manylinux_2_17_x86_64.whl
polyplug-0.1.0-py3-none-macosx_10_9_x86_64.whl
polyplug-0.1.0-py3-none-macosx_11_0_arm64.whl
polyplug-0.1.0-py3-none-win_amd64.whl

Each wheel contains:
polyplug/
├── __init__.py
├── _native/
│   └── libpolyplug.so (platform-specific)
└── ...
```
- Python code: `ctypes.CDLL(os.path.join(os.path.dirname(__file__), "_native", "libpolyplug.so"))`

#### npm / jsr.io
```
@polyplug/runtime/
├── mod.ts
├── polyplug.ts
├── _native/
│   ├── linux-x64/libpolyplug.so
│   ├── darwin-x64/libpolyplug.dylib
│   ├── darwin-arm64/libpolyplug.dylib
│   └── win32-x64/polyplug.dll
└── ...
```
- JS code: `Deno.build.os + "-" + Deno.build.arch` → load correct binary

#### LuaRocks
```
polyplug/
├── polyplug.lua
├── _native/
│   ├── linux-x64/libpolyplug.so
│   ├── darwin-x64/libpolyplug.dylib
│   ├── darwin-arm64/libpolyplug.dylib
│   └── win32-x64/polyplug.dll
└── ...
```
- Lua code: `jit.os .. "-" .. jit.arch` → load correct binary

#### C++
```
CMake FetchContent or find_package:
1. Check for libpolyplug in system paths
2. Check for embedded _native/ folder
3. Download from GitHub Releases if not found
```

---

## Implementation Tasks

### Task 1: GitHub Actions Workflow

Create `.github/workflows/build-native.yml`:
- Matrix build for 4 platforms
- Upload artifacts for each platform
- Create release with all native libraries

### Task 2: NuGet Package Updates

Update `host-libs/csharp/Polyplug/Polyplug.csproj`:
- Add `<Content>` items for runtimes/ folder
- Configure to include native libs in package

### Task 3: Python Package Updates

Update `host-libs/python/`:
- Create `_native/` folder structure
- Update `polyplug/__init__.py` to load from `_native/`
- Update `pyproject.toml` for platform-specific wheels

### Task 4: npm/jsr.io Package Updates

Update `host-libs/js/`:
- Create `_native/` folder structure
- Update `polyplug.ts` to detect platform and load correct binary
- Update `package.json` to include native libs

### Task 5: LuaRocks Package Updates

Update `host-libs/lua/`:
- Create `_native/` folder structure
- Update `polyplug.lua` to detect platform and load correct binary
- Update `.rockspec` to include native libs

### Task 6: C++ Package Updates

Update `host-libs/cpp/`:
- Create CMake config for finding/downloading native lib
- Update documentation

### Task 7: Justfile Updates

Update `justfile`:
- Add `build-native-ci` recipe for CI
- Update `release` to use CI artifacts
- Add `download-native-libs` recipe for local development

### Task 8: Host Library Code Updates

Update each host library to:
- Detect current platform
- Load native library from embedded location
- Fall back to system path if not found

### Task 9: Documentation Updates

Update README and host-lib READMEs:
- Document cross-platform support
- Remove "build from source" requirements
- Update installation instructions

### Task 10: Local Testing

Test on Linux:
- Build native library
- Place in `_native/linux-x64/`
- Verify Python/npm/Lua can load it

---

## File Changes Summary

### New Files
- `.github/workflows/build-native.yml` - CI workflow
- `host-libs/python/polyplug/_native/` - Native lib folder
- `host-libs/js/_native/` - Native lib folder
- `host-libs/lua/_native/` - Native lib folder
- `host-libs/cpp/cmake/FindPolyplug.cmake` - CMake finder

### Modified Files
- `host-libs/csharp/Polyplug/Polyplug.csproj` - Embed runtimes/
- `host-libs/python/polyplug/__init__.py` - Load from _native/
- `host-libs/python/pyproject.toml` - Platform wheels
- `host-libs/js/polyplug.ts` - Platform detection
- `host-libs/js/package.json` - Include _native/
- `host-libs/lua/polyplug.lua` - Platform detection
- `host-libs/lua/*.rockspec` - Include _native/
- `justfile` - CI-based release
- `README.md` - Cross-platform docs

---

## Verification Checklist

- [ ] GitHub Actions builds native libs for all 4 platforms
- [ ] NuGet package includes runtimes/ with all native libs
- [ ] Python wheel includes _native/ with correct lib
- [ ] npm package includes _native/ with all platform libs
- [ ] LuaRocks includes _native/ with all platform libs
- [ ] C++ CMake can find/download native lib
- [ ] Local Linux test: Python can load embedded lib
- [ ] Local Linux test: Deno can load embedded lib
- [ ] Local Linux test: Lua can load embedded lib
- [ ] Documentation updated