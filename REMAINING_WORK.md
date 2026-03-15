# Remaining Work Plan for Polyplug Real Examples

## Current Status

- ✅ Rust Host: Fully working with generated bindings
- ✅ C++ Host: Builds successfully with generated constants  
- ⚠️ C# Host: Generator needs fixes (uses Polyplug.Guest in host code)
- ⏳ Python/Lua/Deno Hosts: Need generator fixes
- ⏳ Guest Plugins: Need to implement new 5-contract API

## Priority Order

### Phase 1: Fix C# Generator (HIGH PRIORITY)
- Fix to not use Polyplug.Guest in host code
- Add proper imports for host types
- Add contract ID constants

### Phase 2: Fix Python/Lua/Deno Generators
- Remove polyplug_guest imports from host code
- Add ContractError classes
- Add contract ID constants

### Phase 3: Update Guest Plugins to 5-Contract API
- Create Decoder, Transformer, Encoder, Reporter, Validator plugins
- Each implements its contract function
- Update bundle.toml files

### Phase 4: Update golden.txt
- Run all hosts and capture expected output
- Update verify_hosts.sh

### Phase 5: Add C++ Scanner
- Add scan_dir() to C++ host-libs
- Update C++ host to discover plugins

## Key Files

Generators to fix:
- crates/polyplug_codegen/src/generators/csharp.rs
- crates/polyplug_codegen/src/generators/python.rs
- crates/polyplug_codegen/src/generators/lua.rs
- crates/polyplug_codegen/src/generators/js_deno.rs

Hosts to refactor:
- examples/hosts/csharp/Program.cs
- examples/hosts/python/host.py
- examples/hosts/lua/host.lua
- examples/hosts/js_deno/host.ts

Guests to create:
- examples/guests/<lang>/decoder/
- examples/guests/<lang>/encoder/
- examples/guests/<lang>/validator/
(Transformer and Reporter exist but need updates)

## Reference Implementation

The Rust host demonstrates the complete pattern:
- Uses generated contract IDs from polyplug_generated namespace
- Uses generated contract callers (PipelineDecoderContract, etc.)
- Type-safe calls with ContractError handling
- Zero hand-written IDs, zero manual dispatch

Follow this pattern for other languages.
