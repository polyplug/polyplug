# Releasing a New Version

This document describes how to release a new version of polyplug.

## Prerequisites

### GitHub Secrets

Set up the following secrets in your GitHub repository settings (Settings → Secrets and variables → Actions):

| Secret | Registry | How to Get |
|--------|----------|------------|
| `CARGO_REGISTRY_TOKEN` | crates.io | `cargo login` → copy token from `~/.cargo/credentials` |
| `NUGET_API_KEY` | nuget.org | nuget.org → Account → API Keys → Create |
| `PYPI_API_TOKEN` | pypi.org | pypi.org → Account settings → API tokens |
| `NPM_TOKEN` | npmjs.com | npmjs.com → Access Tokens → Generate New Token |
| `JSR_TOKEN` | jsr.io | jsr.io → Settings → Tokens |
| `LUAROCKS_API_KEY` | luarocks.org | luarocks.org → Settings → API keys |

## Release Process

### 1. Update Version

Update the version in all relevant files:

```bash
# Update Cargo.toml files (workspace version)
sed -i 's/version = "0.1.0"/version = "0.2.0"/g' Cargo.toml

# Update host library versions
sed -i 's/<Version>0.1.0</<Version>0.2.0</g' sdks/csharp/host/Polyplug/Polyplug.csproj
sed -i 's/<Version>0.1.0</<Version>0.2.0</g' sdks/csharp/host/Loaders/*/*.csproj
sed -i 's/version = "0.1.0"/version = "0.2.0"/g' sdks/python/host/pyproject.toml
sed -i 's/version = "0.1.0"/version = "0.2.0"/g' sdks/python/host/loaders/*/pyproject.toml
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/g' sdks/js/host/package.json
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/g' sdks/js/host/loaders/@polyplug/*/package.json
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/g' sdks/js/guest/package.json
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/g' sdks/python/guest/pyproject.toml

# Update LuaRocks versions (format: version-revision)
# First update content, then rename files
sed -i 's/version = "0.1.0-1"/version = "0.2.0-1"/g' sdks/lua/host/*.rockspec
sed -i 's/version = "0.1.0-1"/version = "0.2.0-1"/g' sdks/lua/host/loaders/*/*.rockspec
sed -i 's/version = "0.1.0-1"/version = "0.2.0-1"/g' sdks/lua/guest/*.rockspec
mv sdks/lua/host/polyplug-0.1.0-1.rockspec sdks/lua/host/polyplug-0.2.0-1.rockspec
mv sdks/lua/guest/polyplug-guest-0.1.0-1.rockspec sdks/lua/guest/polyplug-guest-0.2.0-1.rockspec
for loader in native python lua js dotnet; do
  mv sdks/lua/host/loaders/polyplug-loaders-$loader/polyplug-loaders-$loader-0.1.0-1.rockspec \
     sdks/lua/host/loaders/polyplug-loaders-$loader/polyplug-loaders-$loader-0.2.0-1.rockspec 2>/dev/null || true
done
```

### 2. Commit and Tag

```bash
git add .
git commit -m "chore: release v0.2.0"
git tag v0.2.0
git push origin main --tags
```

### 3. CI Automatically

When you push a tag starting with `v`, the `release.yml` workflow automatically:

1. **Builds native libraries** for all platforms (5 libraries × 4 platforms):
   - `libpolyplug.so/.dylib/.dll` - Core runtime
   - `libpolyplug_native.so/.dylib/.dll` - Native loader
   - `libpolyplug_python.so/.dylib/.dll` - Python loader
   - `libpolyplug_lua.so/.dylib/.dll` - Lua loader
   - `libpolyplug_js.so/.dylib/.dll` - JS/QuickJS loader
   - `libpolyplug_dotnet.so/.dylib/.dll` - .NET loader

   2. **Builds CLI tool** (`polyplugc`) for all platforms

   3. **Tests native library loading** on each platform (Python, Deno, Lua, C#)

   4. **Builds and publishes packages**:
   - **Rust crates** (crates.io): `polyplug_abi`, `polyplug`, `polyplug_guest`, `polyplug_codegen`, `polyplugc`, 5 loader crates
   - **Python** (PyPI): `polyplug`, `polyplug-guest`, 5 loader packages
   - **NuGet**: `Polyplug`, `Polyplug.Guest`, 5 loader packages
   - **npm** (npmjs.com): `@polyplug/runtime`, `@polyplug/guest`, 5 loader packages
   - **jsr.io**: `@polyplug/runtime`, `@polyplug/guest`
   - **LuaRocks**: `polyplug`, `polyplug-guest`, 5 loader packages

5. **Creates GitHub Release** with all artifacts

## Local Testing with `act`

You can test CI workflows locally using [nektos/act](https://github.com/nektos/act):

### Install act

```bash
# Linux
curl -s https://raw.githubusercontent.com/nektos/act/master/install.sh | sudo bash

# macOS
brew install act

# Windows
choco install act-cli
```

### Just Commands for CI Testing

```bash
# List available CI workflows
just ci-list

# Test all workflows locally
just ci-test

# Test a specific workflow
just ci-test ci.yml
just ci-test release.yml

# Test release workflow in dry-run mode
just ci-test-release
```

### Available Just Commands

| Command | Description |
|---------|-------------|
| `just ci-list` | List all available CI workflows |
| `just ci-test` | Run all CI workflows locally using `act` |
| `just ci-test <workflow>` | Run a specific workflow |
| `just ci-test <workflow> <event>` | Run workflow with specific event (push, pull_request, etc.) |
| `just ci-test-release` | Test release workflow in dry-run mode |

## Dry Run

To test the release workflow without publishing:

1. Go to Actions → Release → Run workflow
2. Check "Dry run (build but do not publish)"
3. Click "Run workflow"

This will build everything but skip the publish steps.

## Local Validation

Before releasing, validate locally:

```bash
# Build and test
just build
just test

# Prepare release (without publishing)
just release

# Check dist contents
just dist-info
```

## Adding New Platforms

To add support for a new platform (e.g., Android, iOS, Linux ARM64):

1. Edit `.github/workflows/release.yml`
2. Add a new entry to the `build-native` matrix:

```yaml
- platform: linux-arm64
  os: ubuntu-latest
  target: aarch64-unknown-linux-gnu
  ext: .so
  exe: ''
```

3. Update the `test-native` matrix with the same platform
4. Update the `publish` job to copy the new native library

## Package Structure

After release, users can install polyplug:

### Python
```bash
# Core runtime
pip install polyplug

# Guest library (for plugin authors)
pip install polyplug-guest

# Loaders (pick what you need)
pip install polyplug-loaders-native      # Native .so/.dll plugins
pip install polyplug-loaders-python      # Python plugins
pip install polyplug-loaders-lua         # LuaJIT plugins
pip install polyplug-loaders-js          # QuickJS plugins
pip install polyplug-loaders-dotnet      # .NET plugins
```

### C#/.NET
```bash
# Core runtime
dotnet add package Polyplug

# Guest library (for plugin authors)
dotnet add package Polyplug.Guest

# Loaders (pick what you need)
dotnet add package Polyplug.Loaders.Native
dotnet add package Polyplug.Loaders.Python
dotnet add package Polyplug.Loaders.Lua
dotnet add package Polyplug.Loaders.Js
dotnet add package Polyplug.Loaders.Dotnet
```

### Node.js/Deno
```bash
# Core runtime
npm install @polyplug/runtime
# or
deno add @polyplug/runtime

# Guest library (for plugin authors)
npm install @polyplug/guest

# Loaders (pick what you need)
npm install @polyplug/loaders-native
npm install @polyplug/loaders-python
npm install @polyplug/loaders-lua
npm install @polyplug/loaders-js
npm install @polyplug/loaders-dotnet
```

### Lua
```bash
# Core runtime
luarocks install polyplug

# Guest library (for plugin authors)
luarocks install polyplug-guest

# Loaders available in LuaRocks
```

### Rust
```toml
[dependencies]
polyplug = "0.2.0"           # Core runtime
polyplug_guest = "0.2.0"     # Guest library
polyplug_codegen = "0.2.0"   # Code generation

# Loaders (pick what you need)
polyplug_native = "0.2.0"    # Native .so/.dll plugins
polyplug_python = "0.2.0"    # Python plugins
polyplug_lua = "0.2.0"       # LuaJIT plugins
polyplug_js = "0.2.0"        # QuickJS plugins
polyplug_dotnet = "0.2.0"    # .NET plugins
```

### CLI Tool

Quick install via curl:

```bash
# Linux/macOS
curl -fsSL https://polyplug.github.io/install.sh | bash

# Windows (PowerShell)
powershell -c "irm https://polyplug.github.io/install.ps1 | iex"
```

Or install via cargo:

```bash
cargo install polyplugc
```

Or download the binary directly from GitHub Release assets:
- `polyplugc-linux-x64`
- `polyplugc-macos-x64`
- `polyplugc-macos-arm64`
- `polyplugc-windows-x64.exe`

## Troubleshooting

### CI fails on crates.io publish

- Check that `CARGO_REGISTRY_TOKEN` is valid
- Check that the version doesn't already exist
- Check crate dependencies are published first

### CI fails on NuGet publish

- Check that `NUGET_API_KEY` is valid
- Check that the package doesn't already exist

### CI fails on PyPI publish

- Check that `PYPI_API_TOKEN` is valid
- Check that the version doesn't already exist

### Native library not found

- Ensure the `build-native` job completed successfully
- Check artifacts were uploaded correctly
- Verify the `publish` job downloaded all artifacts

### act fails to run

- Ensure Docker is installed and running
- Check that you have enough disk space for Docker images
- Try running with `act -P ubuntu-latest=node:16-buster-slim` if default image fails