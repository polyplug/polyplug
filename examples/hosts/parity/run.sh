#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$WORKSPACE_DIR"

DEPS_DIR="$WORKSPACE_DIR/target/release/deps"

# Workspace root the harness resolves bundle directories against.
export POLYPLUG_WORKSPACE_DIR="$WORKSPACE_DIR"

export LD_LIBRARY_PATH="$DEPS_DIR:${LD_LIBRARY_PATH:-}"
export POLYPLUG_LIB="$DEPS_DIR/libpolyplug.so"
# Dump a native-level Python traceback if a host or plugin interpreter crashes.
export PYTHONFAULTHANDLER=1

# Loader cdylib paths. Each loader SDK reads POLYPLUG_<LANG>_LIB, falling back to
# the bare soname resolved via LD_LIBRARY_PATH. Set them explicitly so the host
# does not depend on the loader probe order.
export POLYPLUG_NATIVE_LIB="$DEPS_DIR/libpolyplug_native.so"
export POLYPLUG_LUA_LIB="$DEPS_DIR/libpolyplug_lua.so"
export POLYPLUG_JS_LIB="$DEPS_DIR/libpolyplug_js.so"
export POLYPLUG_PYTHON_LIB="$DEPS_DIR/libpolyplug_python.so"

exec "$WORKSPACE_DIR/target/release/parity_host"
