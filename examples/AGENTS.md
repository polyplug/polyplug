# AGENTS.md — polyplug Examples

## Build Script Rules

1. **Use polyplugc for everything** - The build script must NOT:
   - Write manifest files manually
   - Hardcode bundle IDs, contract names, or any values
   - Parse bundle.toml or manifest.toml files manually

2. **Generated files are the source of truth**:
   - `polyplugc generate` creates `generated/manifest.toml` - copy it as-is
   - `polyplugc generate` creates `generated/guest/` bindings - use them
   - Never modify generated files

3. **Build script responsibilities**:
   - Call `polyplugc generate` for each bundle
   - Build the plugin (cargo, g++, etc.)
   - Copy generated manifest.toml to output directory
   - Copy built artifacts (.so, .py, .lua, .js) to output directory

4. **No manual parsing**:
   - Don't use grep/sed/awk on bundle.toml
   - Don't construct manifest.toml with cat/heredoc
   - Let polyplugc do all the work

## Example Correct Build Step

```bash
# Generate code and manifest
polyplugc generate --bundle bundle.toml --lang rust --out generated

# Build the plugin
cargo build --release

# Copy generated manifest (not handwritten!)
cp generated/manifest.toml "$output_dir/"

# Copy built artifact
cp target/release/libplugin.so "$output_dir/"
```