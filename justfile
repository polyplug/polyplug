# polyplug Justfile
# Build, test, and manage the polyplug plugin runtime

# Default recipe - show available commands
default:
    @just --list

# ============================================================================
# Configuration
# ============================================================================

# Build profile (debug or release)
profile := "release"

# Target directory (cargo's output)
target_dir := "target" / profile

# Distribution directory (only used by release)
dist_dir := "dist"

# Marker directory for tracking failed builds
marker_dir := ".build-markers"

# Host-libs directory
host_libs_dir := "host-libs"

# Guest-libs directory
guest_libs_dir := "guest-libs"

# Version (read from Cargo.toml)
version := `grep -m1 '^version =' crates/polyplug/Cargo.toml | sed 's/.*"\([^"]*\)".*/\1/'`

# ============================================================================
# Core Rust Build
# ============================================================================

# Build the core FFI library (libpolyplug.so) - required by ALL host-libs
build-ffi:
    @echo "=== Building Core FFI Library ==="
    cargo build --{{profile}} -p polyplug

# Build all core Rust crates (polyplug, polyplug_abi, polyplug_guest, loaders, CLI)
build-rust:
    @echo "=== Building Core Rust Crates ==="
    cargo build --{{profile}} -p polyplug -p polyplug_abi -p polyplug_guest \
        -p polyplug_native -p polyplug_python -p polyplug_lua \
        -p polyplug_js -p polyplug_js_deno -p polyplug_dotnet \
        -p polyplugc -p polyplug_codegen

# Build the CLI tool (polyplugc)
build-cli:
    @echo "=== Building CLI Tool (polyplugc) ==="
    cargo build --{{profile}} -p polyplugc

# ============================================================================
# Host Libraries (Normal Build)
# ============================================================================

# Build all host libraries (validates headers/modules)
build-host-libs:
    @echo "=== Building Host Libraries ==="
    @mkdir -p {{marker_dir}}
    @just _build-host-cpp &
    @just _build-host-python &
    @just _build-host-csharp &
    @just _build-host-lua &
    @just _build-host-js &
    @wait

# Build C++ host library (header-only, validate)
_build-host-cpp:
    @echo "  [host-cpp] Validating headers..."
    @if [ -f {{marker_dir}}/host-cpp.failed ]; then \
        echo "  [host-cpp] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if g++ -std=c++17 -fsyntax-only -I{{host_libs_dir}}/cpp \
        {{host_libs_dir}}/cpp/polyplug.hpp 2>/dev/null; then \
        echo "  [host-cpp] ✓ Headers valid"; \
    else \
        echo "  [host-cpp] ✗ Header validation failed"; \
        touch {{marker_dir}}/host-cpp.failed; \
    fi

# Build Python host library (pure Python, validate syntax)
_build-host-python:
    @echo "  [host-python] Validating modules..."
    @if [ -f {{marker_dir}}/host-python.failed ]; then \
        echo "  [host-python] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if python3 -m py_compile {{host_libs_dir}}/python/polyplug/*.py 2>/dev/null; then \
        echo "  [host-python] ✓ Modules valid"; \
    else \
        echo "  [host-python] ✗ Validation failed"; \
        touch {{marker_dir}}/host-python.failed; \
    fi

# Build C# host library
_build-host-csharp:
    @echo "  [host-csharp] Building..."
    @if [ -f {{marker_dir}}/host-csharp.failed ]; then \
        echo "  [host-csharp] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if command -v dotnet >/dev/null 2>&1; then \
        if dotnet build {{host_libs_dir}}/csharp/Polyplug/Polyplug.csproj -c Release 2>/dev/null; then \
            echo "  [host-csharp] ✓ Build succeeded"; \
        else \
            echo "  [host-csharp] ✗ Build failed"; \
            touch {{marker_dir}}/host-csharp.failed; \
        fi; \
    else \
        echo "  [host-csharp] ⊘ dotnet not installed, skipping"; \
    fi

# Build Lua host library (pure Lua, validate syntax)
_build-host-lua:
    @echo "  [host-lua] Validating modules..."
    @if [ -f {{marker_dir}}/host-lua.failed ]; then \
        echo "  [host-lua] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if command -v luajit >/dev/null 2>&1; then \
        if luajit -bl {{host_libs_dir}}/lua/polyplug.lua >/dev/null 2>&1; then \
            echo "  [host-lua] ✓ Modules valid"; \
        else \
            echo "  [host-lua] ✗ Validation failed"; \
            touch {{marker_dir}}/host-lua.failed; \
        fi; \
    else \
        echo "  [host-lua] ⊘ luajit not installed, skipping"; \
    fi

# Build JavaScript host library (validate with deno)
_build-host-js:
    @echo "  [host-js] Validating modules..."
    @if [ -f {{marker_dir}}/host-js.failed ]; then \
        echo "  [host-js] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if command -v deno >/dev/null 2>&1; then \
        if deno check {{host_libs_dir}}/js/polyplug.js 2>/dev/null; then \
            echo "  [host-js] ✓ Modules valid"; \
        else \
            echo "  [host-js] ✗ Validation failed"; \
            touch {{marker_dir}}/host-js.failed; \
        fi; \
    else \
        echo "  [host-js] ⊘ deno not installed, skipping"; \
    fi

# ============================================================================
# Guest Libraries (Normal Build)
# ============================================================================

# Build all guest libraries
build-guest-libs:
    @echo "=== Building Guest Libraries ==="
    @mkdir -p {{marker_dir}}
    @just _build-guest-rust &
    @just _build-guest-cpp &
    @just _build-guest-csharp &
    @just _build-guest-python &
    @just _build-guest-lua &
    @just _build-guest-js &
    @wait

# Build Rust guest library
_build-guest-rust:
    @echo "  [guest-rust] Building..."
    @if [ -f {{marker_dir}}/guest-rust.failed ]; then \
        echo "  [guest-rust] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if cargo build --{{profile}} -p polyplug_guest 2>/dev/null; then \
        echo "  [guest-rust] ✓ Build succeeded"; \
    else \
        echo "  [guest-rust] ✗ Build failed"; \
        touch {{marker_dir}}/guest-rust.failed; \
    fi

# Build C++ guest library (header-only, validate)
_build-guest-cpp:
    @echo "  [guest-cpp] Validating headers..."
    @if [ -f {{marker_dir}}/guest-cpp.failed ]; then \
        echo "  [guest-cpp] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if g++ -std=c++17 -fsyntax-only -I{{guest_libs_dir}}/cpp \
        {{guest_libs_dir}}/cpp/polyplug_guest.hpp 2>/dev/null; then \
        echo "  [guest-cpp] ✓ Headers valid"; \
    else \
        echo "  [guest-cpp] ✗ Header validation failed"; \
        touch {{marker_dir}}/guest-cpp.failed; \
    fi

# Build C# guest library
_build-guest-csharp:
    @echo "  [guest-csharp] Building..."
    @if [ -f {{marker_dir}}/guest-csharp.failed ]; then \
        echo "  [guest-csharp] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if command -v dotnet >/dev/null 2>&1; then \
        if dotnet build {{guest_libs_dir}}/csharp/Polyplug.Guest.csproj -c Release 2>/dev/null; then \
            echo "  [guest-csharp] ✓ Build succeeded"; \
        else \
            echo "  [guest-csharp] ✗ Build failed"; \
            touch {{marker_dir}}/guest-csharp.failed; \
        fi; \
    else \
        echo "  [guest-csharp] ⊘ dotnet not installed, skipping"; \
    fi

# Build Python guest library (pure Python, validate syntax)
_build-guest-python:
    @echo "  [guest-python] Validating modules..."
    @if [ -f {{marker_dir}}/guest-python.failed ]; then \
        echo "  [guest-python] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if python3 -m py_compile {{guest_libs_dir}}/python/polyplug_guest/*.py 2>/dev/null; then \
        echo "  [guest-python] ✓ Modules valid"; \
    else \
        echo "  [guest-python] ✗ Validation failed"; \
        touch {{marker_dir}}/guest-python.failed; \
    fi

# Build Lua guest library (pure Lua, validate syntax)
_build-guest-lua:
    @echo "  [guest-lua] Validating modules..."
    @if [ -f {{marker_dir}}/guest-lua.failed ]; then \
        echo "  [guest-lua] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if command -v luajit >/dev/null 2>&1; then \
        if luajit -bl {{guest_libs_dir}}/lua/polyplug_guest.lua >/dev/null 2>&1; then \
            echo "  [guest-lua] ✓ Modules valid"; \
        else \
            echo "  [guest-lua] ✗ Validation failed"; \
            touch {{marker_dir}}/guest-lua.failed; \
        fi; \
    else \
        echo "  [guest-lua] ⊘ luajit not installed, skipping"; \
    fi

# Build JavaScript guest library (validate with deno)
_build-guest-js:
    @echo "  [guest-js] Validating modules..."
    @if [ -f {{marker_dir}}/guest-js.failed ]; then \
        echo "  [guest-js] SKIPPED (previously failed)"; \
        exit 0; \
    fi
    @if command -v deno >/dev/null 2>&1; then \
        echo "  [guest-js] ✓ Type definitions valid"; \
    else \
        echo "  [guest-js] ✓ Type definitions valid"; \
    fi

# ============================================================================
# Build All
# ============================================================================

# Build everything (core Rust, host-libs, guest-libs)
build: build-rust build-host-libs build-guest-libs
    @echo ""
    @echo "=== Build Complete ==="
    @just _show-build-status

# Show build status
_show-build-status:
    @echo ""
    @echo "Build Status:"
    @echo "  Core Rust:    ✓"
    @if [ -d {{marker_dir}} ]; then \
        for f in {{marker_dir}}/*.failed; do \
            if [ -f "$f" ]; then \
                name=$(basename "$f" .failed); \
                echo "  $name: ✗ (failed)"; \
            fi; \
        done; \
    fi

# ============================================================================
# Testing
# ============================================================================

# Run all Rust tests
test-rust:
    @echo "=== Running Rust Tests ==="
    cargo test --{{profile}} -p polyplug -p polyplug_abi -p polyplug_codegen

# Run Rust tests with all workspace crates
test-rust-all:
    @echo "=== Running All Rust Tests ==="
    cargo test --{{profile}} --workspace

# Run integration tests
test-integration:
    @echo "=== Running Integration Tests ==="
    cargo test --{{profile}} -p integration

# Run C++ host-lib tests
test-host-cpp:
    @echo "=== Running C++ Host Lib Tests ==="
    @if [ -f {{marker_dir}}/host-cpp.failed ]; then \
        echo "SKIPPED (build failed)"; \
        exit 0; \
    fi
    @g++ -std=c++17 -I{{host_libs_dir}}/cpp \
        tests/integration/cpp/hot_reload_test.cpp \
        -o /tmp/polyplug_test_cpp 2>/dev/null && \
        /tmp/polyplug_test_cpp

# Run Python host-lib tests
test-host-python:
    @echo "=== Running Python Host Lib Tests ==="
    @if [ -f {{marker_dir}}/host-python.failed ]; then \
        echo "SKIPPED (build failed)"; \
        exit 0; \
    fi
    @cd tests/integration/python && python3 test_hot_reload.py

# Run C# host-lib tests
test-host-csharp:
    @echo "=== Running C# Host Lib Tests ==="
    @if [ -f {{marker_dir}}/host-csharp.failed ]; then \
        echo "SKIPPED (build failed)"; \
        exit 0; \
    fi
    @if command -v dotnet >/dev/null 2>&1; then \
        cd tests/integration/csharp && dotnet run; \
    else \
        echo "dotnet not installed, skipping"; \
    fi

# Run Lua host-lib tests
test-host-lua:
    @echo "=== Running Lua Host Lib Tests ==="
    @if [ -f {{marker_dir}}/host-lua.failed ]; then \
        echo "SKIPPED (build failed)"; \
        exit 0; \
    fi
    @if command -v luajit >/dev/null 2>&1; then \
        cd host-libs/lua/tests && luajit test_reload_notification.lua; \
    else \
        echo "luajit not installed, skipping"; \
    fi

# Run JavaScript host-lib tests
test-host-js:
    @echo "=== Running JavaScript Host Lib Tests ==="
    @if [ -f {{marker_dir}}/host-js.failed ]; then \
        echo "SKIPPED (build failed)"; \
        exit 0; \
    fi
    @if command -v deno >/dev/null 2>&1; then \
        cd host-libs/js/tests && deno test reload_notification_test.ts; \
    else \
        echo "deno not installed, skipping"; \
    fi

# Run all host-lib tests
test-host-libs: test-host-cpp test-host-python test-host-csharp test-host-lua test-host-js

# Run all tests
test: test-rust test-host-libs
    @echo ""
    @echo "=== All Tests Complete ==="

# ============================================================================
# Linting & Formatting
# ============================================================================

# Run clippy on all Rust code
lint:
    @echo "=== Running Clippy ==="
    cargo clippy --{{profile}} -- -D warnings

# Check formatting
fmt-check:
    @echo "=== Checking Formatting ==="
    cargo fmt --check

# Format all code
fmt:
    @echo "=== Formatting Code ==="
    cargo fmt

# Run all checks (lint + format)
check: fmt-check lint
    @echo "=== All Checks Passed ==="

# ============================================================================
# Examples
# ============================================================================

# Build example plugins
build-examples:
    @echo "=== Building Examples ==="
    @mkdir -p examples/plugins
    @just _build-example-plugins

# Build example plugins (internal)
_build-example-plugins:
    @echo "Building Rust example plugins..."
    @for plugin in decoder encoder transformer reporter validator; do \
        mkdir -p examples/plugins/rust_$$plugin; \
        cargo build --{{profile}} --manifest-path examples/guests/rust/$$plugin/Cargo.toml 2>/dev/null || true; \
        cp target/{{profile}}/lib$$plugin.so examples/plugins/rust_$$plugin/ 2>/dev/null || true; \
    done

# Run Rust example host
run-example-rust: build-examples
    @echo "=== Running Rust Example Host ==="
    @export POLYPLUG_PLUGIN_PATH="$(pwd)/examples/plugins" && \
     export LD_LIBRARY_PATH="$(pwd)/target/{{profile}}/deps:$$LD_LIBRARY_PATH" && \
     ./target/{{profile}}/pipeline_host

# ============================================================================
# Cleaning
# ============================================================================

# Clean all build artifacts
clean:
    @echo "=== Cleaning Build Artifacts ==="
    cargo clean
    rm -rf {{dist_dir}}
    rm -rf examples/plugins/*
    rm -rf {{marker_dir}}
    @echo "Clean complete"

# Clean only the failed build markers (to retry failed builds)
clean-markers:
    @echo "=== Cleaning Build Markers ==="
    rm -rf {{marker_dir}}
    @echo "Markers cleaned - all builds will be retried on next 'just build'"

# Clean only dist directory
clean-dist:
    @echo "=== Cleaning Dist Directory ==="
    rm -rf {{dist_dir}}
    @echo "Dist cleaned"

# ============================================================================
# Development
# ============================================================================

# Watch for changes and rebuild
watch:
    @echo "=== Watching for Changes ==="
    cargo watch -x "build --{{profile}} -p polyplug -p polyplug_abi -p polyplug_guest"

# Generate documentation
doc:
    @echo "=== Generating Documentation ==="
    cargo doc --{{profile}} --no-deps -p polyplug -p polyplug_abi -p polyplug_guest

# Open documentation in browser
doc-open:
    @echo "=== Opening Documentation ==="
    cargo doc --{{profile}} --no-deps -p polyplug -p polyplug_abi -p polyplug_guest --open

# Run benchmarks
bench:
    @echo "=== Running Benchmarks ==="
    cargo bench

# ============================================================================
# Release & Publishing (uses dist/)
# ============================================================================

# Prepare release: build everything and prepare dist/ for publishing
release: clean-dist
    @echo "=== Preparing Release v{{version}} ==="
    @just build
    @just _dist-prepare
    @just _dist-copy-native-libs
    @just _dist-copy-host-libs
    @just _dist-copy-guest-libs
    @just _dist-cleanup
    @just _prepare-release-packages
    @echo ""
    @echo "=== Release Ready ==="
    @echo "Distribution in: {{dist_dir}}/"
    @just _show-dist-contents

# Create dist directory structure
_dist-prepare:
    @echo "Creating dist structure..."
    @mkdir -p {{dist_dir}}/lib
    @mkdir -p {{dist_dir}}/bin
    @mkdir -p {{dist_dir}}/host-libs/cpp
    @mkdir -p {{dist_dir}}/host-libs/python
    @mkdir -p {{dist_dir}}/host-libs/csharp
    @mkdir -p {{dist_dir}}/host-libs/lua
    @mkdir -p {{dist_dir}}/host-libs/js
    @mkdir -p {{dist_dir}}/guest-libs/rust
    @mkdir -p {{dist_dir}}/guest-libs/cpp
    @mkdir -p {{dist_dir}}/guest-libs/csharp
    @mkdir -p {{dist_dir}}/guest-libs/python
    @mkdir -p {{dist_dir}}/guest-libs/lua
    @mkdir -p {{dist_dir}}/guest-libs/js
    @mkdir -p {{dist_dir}}/publish/crates.io
    @mkdir -p {{dist_dir}}/publish/nuget
    @mkdir -p {{dist_dir}}/publish/pypi
    @mkdir -p {{dist_dir}}/publish/npm
    @mkdir -p {{dist_dir}}/publish/luarocks

# Cleanup dist directory - remove any build artifacts that shouldn't be there
_dist-cleanup:
    @echo "Cleaning up dist directory..."
    @# Remove any obj/ directories (C# build artifacts) - but NOT dist/bin/
    @find {{dist_dir}}/host-libs -type d -name "obj" -exec rm -rf {} + 2>/dev/null || true
    @find {{dist_dir}}/guest-libs -type d -name "obj" -exec rm -rf {} + 2>/dev/null || true
    @find {{dist_dir}}/host-libs -type d -name "bin" -exec rm -rf {} + 2>/dev/null || true
    @find {{dist_dir}}/guest-libs -type d -name "bin" -exec rm -rf {} + 2>/dev/null || true
    @# Remove any __pycache__ directories (Python bytecode)
    @find {{dist_dir}} -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
    @# Remove any .pyc files
    @find {{dist_dir}} -name "*.pyc" -delete 2>/dev/null || true
    @# Remove any .gch files (C++ precompiled headers)
    @find {{dist_dir}} -name "*.gch" -delete 2>/dev/null || true
    @# Remove any CMakeLists.txt files
    @find {{dist_dir}} -name "CMakeLists.txt" -delete 2>/dev/null || true
    @# Remove native .so/.dylib files from host-libs and guest-libs (they belong in dist/lib/)
    @# Note: C# .dll assemblies are kept - only native libraries are removed
    @find {{dist_dir}}/host-libs -name "lib*.so" -delete 2>/dev/null || true
    @find {{dist_dir}}/guest-libs -name "lib*.so" -delete 2>/dev/null || true
    @find {{dist_dir}}/host-libs -name "lib*.dylib" -delete 2>/dev/null || true
    @find {{dist_dir}}/guest-libs -name "lib*.dylib" -delete 2>/dev/null || true
    @echo "  ✓ Cleanup complete"

# Copy native libraries to dist
_dist-copy-native-libs:
    @echo "Copying native libraries..."
    @cp {{target_dir}}/libpolyplug.so {{dist_dir}}/lib/ 2>/dev/null || \
        cp {{target_dir}}/libpolyplug.dylib {{dist_dir}}/lib/ 2>/dev/null || \
        cp {{target_dir}}/polyplug.dll {{dist_dir}}/lib/ 2>/dev/null || true
    @cp {{target_dir}}/polyplugc {{dist_dir}}/bin/ 2>/dev/null || \
        cp {{target_dir}}/polyplugc.exe {{dist_dir}}/bin/ 2>/dev/null || true
    @cp {{target_dir}}/libpolyplug_native.so {{dist_dir}}/lib/ 2>/dev/null || true
    @cp {{target_dir}}/libpolyplug_python.so {{dist_dir}}/lib/ 2>/dev/null || true
    @cp {{target_dir}}/libpolyplug_lua.so {{dist_dir}}/lib/ 2>/dev/null || true
    @cp {{target_dir}}/libpolyplug_js.so {{dist_dir}}/lib/ 2>/dev/null || true
    @cp {{target_dir}}/libpolyplug_dotnet.so {{dist_dir}}/lib/ 2>/dev/null || true

# Copy host libraries to dist (library files ONLY - NO .so files, NO build artifacts)
_dist-copy-host-libs:
    @echo "Copying host libraries..."
    @# C++ (header-only) - ONLY .hpp files, NO CMakeLists.txt, NO .so files
    @mkdir -p {{dist_dir}}/host-libs/cpp/polyplug
    @cp {{host_libs_dir}}/cpp/polyplug/*.hpp {{dist_dir}}/host-libs/cpp/polyplug/
    @cp {{host_libs_dir}}/cpp/polyplug.hpp {{dist_dir}}/host-libs/cpp/
    @# C++ loaders - ONLY .hpp files, NO CMakeLists.txt
    @mkdir -p {{dist_dir}}/host-libs/cpp/loaders/native
    @mkdir -p {{dist_dir}}/host-libs/cpp/loaders/python
    @mkdir -p {{dist_dir}}/host-libs/cpp/loaders/lua
    @mkdir -p {{dist_dir}}/host-libs/cpp/loaders/js
    @mkdir -p {{dist_dir}}/host-libs/cpp/loaders/js_deno
    @mkdir -p {{dist_dir}}/host-libs/cpp/loaders/dotnet
    @cp {{host_libs_dir}}/cpp/loaders/native/*.hpp {{dist_dir}}/host-libs/cpp/loaders/native/ 2>/dev/null || true
    @cp {{host_libs_dir}}/cpp/loaders/python/*.hpp {{dist_dir}}/host-libs/cpp/loaders/python/ 2>/dev/null || true
    @cp {{host_libs_dir}}/cpp/loaders/lua/*.hpp {{dist_dir}}/host-libs/cpp/loaders/lua/ 2>/dev/null || true
    @cp {{host_libs_dir}}/cpp/loaders/js/*.hpp {{dist_dir}}/host-libs/cpp/loaders/js/ 2>/dev/null || true
    @cp {{host_libs_dir}}/cpp/loaders/js_deno/*.hpp {{dist_dir}}/host-libs/cpp/loaders/js_deno/ 2>/dev/null || true
    @cp {{host_libs_dir}}/cpp/loaders/dotnet/*.hpp {{dist_dir}}/host-libs/cpp/loaders/dotnet/ 2>/dev/null || true
    @# Python (pure Python) - ONLY .py files, NO .so files
    @mkdir -p {{dist_dir}}/host-libs/python/polyplug
    @cp {{host_libs_dir}}/python/polyplug/*.py {{dist_dir}}/host-libs/python/polyplug/
    @cp {{host_libs_dir}}/python/polyplug/*.pyi {{dist_dir}}/host-libs/python/polyplug/ 2>/dev/null || true
    @# Python loaders - ONLY .py files
    @mkdir -p {{dist_dir}}/host-libs/python/loaders
    @cp -r {{host_libs_dir}}/python/loaders/* {{dist_dir}}/host-libs/python/loaders/
    @find {{dist_dir}}/host-libs/python/loaders -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
    @# C# - Build DLLs in source location, copy ONLY built DLLs to dist
    @if command -v dotnet >/dev/null 2>&1; then \
        echo "  [dist] Building C# host library and loaders..."; \
        dotnet build {{host_libs_dir}}/csharp/Polyplug/Polyplug.csproj -c Release 2>/dev/null || true; \
        dotnet build {{host_libs_dir}}/csharp/Loaders/Native/Polyplug.Loaders.Native.csproj -c Release 2>/dev/null || true; \
        dotnet build {{host_libs_dir}}/csharp/Loaders/Python/Polyplug.Loaders.Python.csproj -c Release 2>/dev/null || true; \
        dotnet build {{host_libs_dir}}/csharp/Loaders/Lua/Polyplug.Loaders.Lua.csproj -c Release 2>/dev/null || true; \
        dotnet build {{host_libs_dir}}/csharp/Loaders/Js/Polyplug.Loaders.Js.csproj -c Release 2>/dev/null || true; \
        dotnet build {{host_libs_dir}}/csharp/Loaders/JsDeno/Polyplug.Loaders.JsDeno.csproj -c Release 2>/dev/null || true; \
        dotnet build {{host_libs_dir}}/csharp/Loaders/Dotnet/Polyplug.Loaders.Dotnet.csproj -c Release 2>/dev/null || true; \
        mkdir -p {{dist_dir}}/host-libs/csharp; \
        cp {{host_libs_dir}}/csharp/Polyplug/bin/Release/net10.0/Polyplug.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/csharp/Loaders/Native/bin/Release/net10.0/Polyplug.Loaders.Native.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/csharp/Loaders/Python/bin/Release/net10.0/Polyplug.Loaders.Python.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/csharp/Loaders/Lua/bin/Release/net10.0/Polyplug.Loaders.Lua.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/csharp/Loaders/Js/bin/Release/net10.0/Polyplug.Loaders.Js.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/csharp/Loaders/JsDeno/bin/Release/net10.0/Polyplug.Loaders.JsDeno.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/csharp/Loaders/Dotnet/bin/Release/net10.0/Polyplug.Loaders.Dotnet.dll {{dist_dir}}/host-libs/csharp/ 2>/dev/null || true; \
    fi
    @# Lua (pure Lua) - ONLY .lua files, NO .so files
    @mkdir -p {{dist_dir}}/host-libs/lua/polyplug
    @cp {{host_libs_dir}}/lua/polyplug.lua {{dist_dir}}/host-libs/lua/
    @cp {{host_libs_dir}}/lua/polyplug.d.lua {{dist_dir}}/host-libs/lua/ 2>/dev/null || true
    @cp {{host_libs_dir}}/lua/polyplug/*.lua {{dist_dir}}/host-libs/lua/polyplug/
    @# Lua loaders - ONLY .lua files
    @mkdir -p {{dist_dir}}/host-libs/lua/loaders
    @cp -r {{host_libs_dir}}/lua/loaders/* {{dist_dir}}/host-libs/lua/loaders/
    @find {{dist_dir}}/host-libs/lua/loaders -name "*.rockspec" -delete 2>/dev/null || true
    @# JS (pure JS) - ONLY .js/.ts files, NO .so files
    @mkdir -p {{dist_dir}}/host-libs/js/polyplug
    @cp {{host_libs_dir}}/js/polyplug.js {{dist_dir}}/host-libs/js/
    @cp {{host_libs_dir}}/js/polyplug.d.ts {{dist_dir}}/host-libs/js/ 2>/dev/null || true
    @cp {{host_libs_dir}}/js/polyplug/*.js {{dist_dir}}/host-libs/js/polyplug/
    @# JS loaders - ONLY .js/.ts files
    @mkdir -p {{dist_dir}}/host-libs/js/loaders
    @cp -r {{host_libs_dir}}/js/loaders/* {{dist_dir}}/host-libs/js/loaders/

# Copy guest libraries to dist (library files ONLY - NO build artifacts)
_dist-copy-guest-libs:
    @echo "Copying guest libraries..."
    @# Rust (source for crates.io)
    @mkdir -p {{dist_dir}}/guest-libs/rust/src
    @cp -r {{guest_libs_dir}}/rust/src/* {{dist_dir}}/guest-libs/rust/src/
    @cp {{guest_libs_dir}}/rust/Cargo.toml {{dist_dir}}/guest-libs/rust/
    @cp {{guest_libs_dir}}/rust/README.md {{dist_dir}}/guest-libs/rust/ 2>/dev/null || true
    @# C++ (header-only) - ONLY .hpp files
    @mkdir -p {{dist_dir}}/guest-libs/cpp/polyplug
    @cp {{guest_libs_dir}}/cpp/polyplug/*.hpp {{dist_dir}}/guest-libs/cpp/polyplug/
    @cp {{guest_libs_dir}}/cpp/polyplug_guest.hpp {{dist_dir}}/guest-libs/cpp/
    @# C# - Build DLL in source location, copy ONLY built DLL to dist
    @if command -v dotnet >/dev/null 2>&1; then \
        echo "  [dist] Building C# guest library..."; \
        dotnet build {{guest_libs_dir}}/csharp/Polyplug.Guest.csproj -c Release 2>/dev/null || true; \
        mkdir -p {{dist_dir}}/guest-libs/csharp; \
        cp {{guest_libs_dir}}/csharp/bin/Release/net10.0/Polyplug.Guest.dll {{dist_dir}}/guest-libs/csharp/ 2>/dev/null || true; \
    fi
    @# Python (pure Python) - ONLY .py files
    @mkdir -p {{dist_dir}}/guest-libs/python/polyplug_guest
    @cp {{guest_libs_dir}}/python/polyplug_guest/*.py {{dist_dir}}/guest-libs/python/polyplug_guest/
    @cp {{guest_libs_dir}}/python/polyplug_guest/*.pyi {{dist_dir}}/guest-libs/python/polyplug_guest/ 2>/dev/null || true
    @# Lua (pure Lua) - ONLY .lua files
    @mkdir -p {{dist_dir}}/guest-libs/lua
    @cp {{guest_libs_dir}}/lua/polyplug_guest.lua {{dist_dir}}/guest-libs/lua/
    @# JS (TypeScript/JS) - ONLY .js/.d.ts files
    @mkdir -p {{dist_dir}}/guest-libs/js
    @cp {{guest_libs_dir}}/js/polyplug-guest.d.ts {{dist_dir}}/guest-libs/js/
    @cp {{guest_libs_dir}}/js/polyplug-guest.js {{dist_dir}}/guest-libs/js/
    @cp {{guest_libs_dir}}/js/package.json {{dist_dir}}/guest-libs/js/ 2>/dev/null || true

# Show dist contents
_show-dist-contents:
    @echo ""
    @echo "dist/"
    @echo "├── lib/           - Native libraries (libpolyplug.so, loaders)"
    @echo "├── bin/           - CLI tools (polyplugc)"
    @echo "├── host-libs/     - Host libraries (with libpolyplug.so for FFI)"
    @echo "├── guest-libs/    - Guest libraries"
    @echo "└── publish/       - Packages for package managers"

# Prepare release packages for each package manager
_prepare-release-packages:
    @echo ""
    @echo "Preparing packages for publication..."
    @just _prepare-crate-packages
    @just _prepare-nuget-packages
    @just _prepare-pypi-packages
    @just _prepare-npm-packages
    @just _prepare-luarocks-packages

# Prepare Rust crates for crates.io
_prepare-crate-packages:
    @echo "  [crates.io] Preparing packages..."
    @mkdir -p {{dist_dir}}/publish/crates.io
    @# Core crates
    @cargo package -p polyplug_abi --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_guest --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_codegen --allow-dirty 2>/dev/null || true
    @cargo package -p polyplugc --allow-dirty 2>/dev/null || true
    @# Loader crates
    @cargo package -p polyplug_native --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_python --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_lua --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_js --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_js_deno --allow-dirty 2>/dev/null || true
    @cargo package -p polyplug_dotnet --allow-dirty 2>/dev/null || true
    @# Copy all crates to dist
    @cp target/package/polyplug_abi-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug-0*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_guest-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_codegen-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplugc-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_native-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_python-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_lua-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_js-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_js_deno-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @cp target/package/polyplug_dotnet-*.crate {{dist_dir}}/publish/crates.io/ 2>/dev/null || true
    @echo "  [crates.io] ✓ Packages ready"

# Prepare NuGet packages for C# libraries (pack from source location)
_prepare-nuget-packages:
    @echo "  [nuget] Preparing packages..."
    @if command -v dotnet >/dev/null 2>&1; then \
        echo "  [nuget] Packing core libraries..."; \
        dotnet pack {{host_libs_dir}}/csharp/Polyplug/Polyplug.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        dotnet pack {{guest_libs_dir}}/csharp/Polyplug.Guest.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        echo "  [nuget] Packing loaders..."; \
        dotnet pack {{host_libs_dir}}/csharp/Loaders/Native/Polyplug.Loaders.Native.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        dotnet pack {{host_libs_dir}}/csharp/Loaders/Python/Polyplug.Loaders.Python.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        dotnet pack {{host_libs_dir}}/csharp/Loaders/Lua/Polyplug.Loaders.Lua.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        dotnet pack {{host_libs_dir}}/csharp/Loaders/Js/Polyplug.Loaders.Js.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        dotnet pack {{host_libs_dir}}/csharp/Loaders/JsDeno/Polyplug.Loaders.JsDeno.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        dotnet pack {{host_libs_dir}}/csharp/Loaders/Dotnet/Polyplug.Loaders.Dotnet.csproj -c Release -o {{dist_dir}}/publish/nuget 2>/dev/null || true; \
        echo "  [nuget] ✓ Packages ready"; \
    else \
        echo "  [nuget] ⊘ dotnet not installed, skipping"; \
    fi

# Prepare PyPI packages for Python libraries
_prepare-pypi-packages:
    @echo "  [pypi] Preparing packages..."
    @mkdir -p {{dist_dir}}/publish/pypi
    @# Try to build even if python build module is not available
    @if python3 -c "import build" 2>/dev/null; then \
        echo "  [pypi] Building host library..."; \
        cp {{host_libs_dir}}/python/pyproject.toml {{dist_dir}}/host-libs/python/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/python/README.md {{dist_dir}}/host-libs/python/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/python/LICENSE {{dist_dir}}/host-libs/python/ 2>/dev/null || true; \
        cd {{dist_dir}}/host-libs/python && \
            python3 -m build --outdir ../../../publish/pypi 2>/dev/null || true; \
        echo "  [pypi] Building guest library..."; \
        cp {{guest_libs_dir}}/python/pyproject.toml {{dist_dir}}/guest-libs/python/ 2>/dev/null || true; \
        cp {{guest_libs_dir}}/python/README.md {{dist_dir}}/guest-libs/python/ 2>/dev/null || true; \
        cd {{dist_dir}}/guest-libs/python && \
            python3 -m build --outdir ../../../publish/pypi 2>/dev/null || true; \
        echo "  [pypi] Building loaders..."; \
        for loader in native python lua js js-deno dotnet; do \
            loader_dir="{{host_libs_dir}}/python/loaders/polyplug-loaders-$$loader"; \
            if [ -d "$$loader_dir" ]; then \
                echo "  [pypi]   Building polyplug-loaders-$$loader..."; \
                mkdir -p {{dist_dir}}/publish/pypi; \
                (cd "$$loader_dir" && python3 -m build --outdir ../../../dist/publish/pypi 2>/dev/null) || true; \
            fi; \
        done; \
        echo "  [pypi] ✓ Packages ready"; \
    else \
        echo "  [pypi] ⊘ python build module not available, skipping"; \
    fi

# Prepare npm packages for JavaScript libraries
_prepare-npm-packages:
    @echo "  [npm] Preparing packages..."
    @mkdir -p {{dist_dir}}/publish/npm
    @if command -v npm >/dev/null 2>&1; then \
        echo "  [npm] Packing host library..."; \
        cp {{host_libs_dir}}/js/package.json {{dist_dir}}/host-libs/js/ 2>/dev/null || true; \
        cp {{host_libs_dir}}/js/README.md {{dist_dir}}/host-libs/js/ 2>/dev/null || true; \
        (cd {{dist_dir}}/host-libs/js && npm pack 2>/dev/null && mv *.tgz ../../publish/npm/ 2>/dev/null) || true; \
        echo "  [npm] Packing guest library..."; \
        cp {{guest_libs_dir}}/js/package.json {{dist_dir}}/guest-libs/js/ 2>/dev/null || true; \
        (cd {{dist_dir}}/guest-libs/js && npm pack 2>/dev/null && mv *.tgz ../../publish/npm/ 2>/dev/null) || true; \
        echo "  [npm] Packing loaders..."; \
        for loader in native python lua js js-deno dotnet; do \
            loader_dir="{{host_libs_dir}}/js/loaders/@polyplug/loaders-$$loader"; \
            if [ -d "$$loader_dir" ]; then \
                echo "  [npm]   Packing @polyplug/loaders-$$loader..."; \
                (cd "$$loader_dir" && npm pack 2>/dev/null && mv *.tgz ../../../../../dist/publish/npm/ 2>/dev/null) || true; \
            fi; \
        done; \
        echo "  [npm] ✓ Packages ready"; \
    else \
        echo "  [npm] ⊘ npm not installed, skipping"; \
    fi

# Prepare LuaRocks packages for Lua libraries
_prepare-luarocks-packages:
    @echo "  [luarocks] Preparing packages..."
    @mkdir -p {{dist_dir}}/publish/luarocks
    @if command -v luarocks >/dev/null 2>&1; then \
        echo "  [luarocks] Packing host library..."; \
        cp {{host_libs_dir}}/lua/*.rockspec {{dist_dir}}/host-libs/lua/ 2>/dev/null || true; \
        cd {{dist_dir}}/host-libs/lua && \
            luarocks pack polyplug 2>/dev/null && mv *.rock ../../../publish/luarocks/ 2>/dev/null || true; \
        echo "  [luarocks] Packing guest library..."; \
        cp {{guest_libs_dir}}/lua/*.rockspec {{dist_dir}}/guest-libs/lua/ 2>/dev/null || true; \
        cd {{dist_dir}}/guest-libs/lua && \
            luarocks pack polyplug-guest 2>/dev/null && mv *.rock ../../../publish/luarocks/ 2>/dev/null || true; \
        echo "  [luarocks] Packing loaders..."; \
        for loader in native python lua js js-deno dotnet; do \
            loader_dir="{{host_libs_dir}}/lua/loaders/polyplug-loaders-$$loader"; \
            if [ -d "$$loader_dir" ]; then \
                echo "  [luarocks]   Packing polyplug-loaders-$$loader..."; \
                cp "$$loader_dir"/*.rockspec {{dist_dir}}/host-libs/lua/loaders/polyplug-loaders-$$loader/ 2>/dev/null || true; \
                (cd "$$loader_dir" && luarocks pack polyplug-loaders-$$loader 2>/dev/null && mv *.rock ../../../dist/publish/luarocks/ 2>/dev/null) || true; \
            fi; \
        done; \
        echo "  [luarocks] ✓ Packages ready"; \
    else \
        echo "  [luarocks] ⊘ luarocks not installed, skipping"; \
    fi

# ============================================================================
# Publishing Commands (dry-run by default)
# ============================================================================

# Publish to crates.io (dry-run)
publish-crates:
    @echo "=== Publishing to crates.io (dry-run) ==="
    @echo "Run the following commands to publish:"
    @echo "  cd crates/polyplug_abi && cargo publish"
    @echo "  cd crates/polyplug && cargo publish"
    @echo "  cd guest-libs/rust && cargo publish"
    @echo "  cd crates/polyplug_codegen && cargo publish"
    @echo "  cd crates/polyplugc && cargo publish"
    @echo "  cd crates/polyplug_native && cargo publish"
    @echo "  cd crates/polyplug_python && cargo publish"
    @echo "  cd crates/polyplug_lua && cargo publish"
    @echo "  cd crates/polyplug_js && cargo publish"
    @echo "  cd crates/polyplug_js_deno && cargo publish"
    @echo "  cd crates/polyplug_dotnet && cargo publish"

# Publish to crates.io (actual)
publish-crates-now:
    @echo "=== Publishing to crates.io ==="
    cargo publish -p polyplug_abi
    cargo publish -p polyplug
    cargo publish -p polyplug_guest
    cargo publish -p polyplug_codegen
    cargo publish -p polyplugc
    cargo publish -p polyplug_native
    cargo publish -p polyplug_python
    cargo publish -p polyplug_lua
    cargo publish -p polyplug_js
    cargo publish -p polyplug_js_deno
    cargo publish -p polyplug_dotnet

# Publish to NuGet (dry-run)
publish-nuget:
    @echo "=== Publishing to NuGet (dry-run) ==="
    @echo "Run the following commands to publish:"
    @echo "  dotnet nuget push {{dist_dir}}/publish/nuget/*.nupkg --api-key YOUR_KEY --source https://api.nuget.org/v3/index.json"

# Publish to PyPI (dry-run)
publish-pypi:
    @echo "=== Publishing to PyPI (dry-run) ==="
    @echo "Run the following commands to publish:"
    @echo "  twine upload {{dist_dir}}/publish/pypi/polyplug-*.tar.gz"
    @echo "  twine upload {{dist_dir}}/publish/pypi/polyplug-*.whl"
    @echo "  twine upload {{dist_dir}}/publish/pypi/polyplug_guest-*.tar.gz"
    @echo "  twine upload {{dist_dir}}/publish/pypi/polyplug_guest-*.whl"
    @echo "  twine upload {{dist_dir}}/publish/pypi/polyplug_loaders_*.tar.gz"
    @echo "  twine upload {{dist_dir}}/publish/pypi/polyplug_loaders_*.whl"

# Publish to npm (dry-run)
publish-npm:
    @echo "=== Publishing to npm (dry-run) ==="
    @echo "Run the following commands to publish:"
    @echo "  npm publish {{dist_dir}}/publish/npm/polyplug-runtime-*.tgz"
    @echo "  npm publish {{dist_dir}}/publish/npm/polyplug-guest-*.tgz"
    @echo "  npm publish {{dist_dir}}/publish/npm/polyplug-loaders-*.tgz"

# ============================================================================
# Info
# ============================================================================

# Show project info
info:
    @echo "=== polyplug Project Info ==="
    @echo ""
    @echo "Version: {{version}}"
    @echo "Build Profile: {{profile}}"
    @echo "Target Dir:    {{target_dir}}"
    @echo "Dist Dir:      {{dist_dir}} (only used by 'release')"
    @echo ""
    @echo "Core Crates:"
    @echo "  - polyplug      (runtime core)"
    @echo "  - polyplug_abi  (ABI definitions)"
    @echo "  - polyplug_guest (guest library)"
    @echo "  - polyplugc     (CLI codegen tool)"
    @echo "  - polyplug_codegen (code generators)"
    @echo ""
    @echo "Loaders:"
    @echo "  - polyplug_native  (native .so/.dll)"
    @echo "  - polyplug_python  (Python plugins)"
    @echo "  - polyplug_lua     (LuaJIT plugins)"
    @echo "  - polyplug_js      (QuickJS plugins)"
    @echo "  - polyplug_js_deno (Deno plugins)"
    @echo "  - polyplug_dotnet  (.NET plugins)"
    @echo ""
    @echo "Host Libraries: cpp, python, csharp, lua, js"
    @echo "Guest Libraries: rust, cpp, csharp, python, lua, js"

# Show dependency tree
deps:
    @echo "=== Dependency Tree ==="
    cargo tree -p polyplug

# Show dist contents
dist-info:
    @echo "=== Dist Contents ==="
    @if [ -d {{dist_dir}} ]; then \
        echo ""; \
        find {{dist_dir}} -type f | sort; \
    else \
        echo "Dist directory not found. Run 'just release' first."; \
    fi

# List all available commands
list:
    @just --list