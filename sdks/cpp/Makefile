# Test runner for the hand-written C++ SDK test programs.
#
# - abi/test_layout.cpp is a compile-time suite (static_assert only, no main):
#   compiling it IS the test — every layout assert is evaluated by the compiler.
# - host/test_reload_runtime.cpp is a REAL-runtime hot-reload test: it links
#   the core + native-loader cdylibs from the workspace target dir and drives
#   a real reload of the tests/fixtures/reload_plugin fixtures. Skip-honestly:
#   when the cdylibs are missing the build FAILS LOUDLY with instructions — a
#   runtime test that silently passes hides exactly the never-run breakage
#   class it exists to catch.
#
# Mirrors examples/hosts/cpp's depfile pattern (-MMD -MP) so header changes
# (abi.hpp, guest.hpp, runtime.hpp) trigger rebuilds.
#
# Run from the repo root with: make -C sdks/cpp test   (or: just test-sdk-cpp)

CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2 -MMD -MP
INCLUDES = -Iabi -Iguest
BUILD_DIR = build

# Workspace cargo output for the reload runtime test. Probed across profiles
# (release first, matching the fixture build); override with
# `make test POLYPLUG_TARGET_DIR=...` to point at a specific build.
POLYPLUG_TARGET_DIR ?= $(patsubst %/,%,$(dir $(firstword $(wildcard \
    ../../target/release/libpolyplug.so ../../target/release/libpolyplug.dylib \
    ../../target/debug/libpolyplug.so ../../target/debug/libpolyplug.dylib))))
FIXTURES_DIR = $(abspath ../../tests/fixtures)

.PHONY: test clean

test: $(BUILD_DIR)/test_layout.o $(BUILD_DIR)/test_signature_policy_config $(BUILD_DIR)/test_internal_plugin_runtime $(BUILD_DIR)/test_reload_runtime
	@echo "OK: abi/test_layout.cpp compiled - all ABI layout static_asserts hold"
	$(BUILD_DIR)/test_signature_policy_config
	$(BUILD_DIR)/test_internal_plugin_runtime
	$(BUILD_DIR)/test_reload_runtime

$(BUILD_DIR)/test_layout.o: abi/test_layout.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c $< -o $@

# Builder setter test — defines local create/destroy stubs that capture the
# marshaled RuntimeConfig, so it needs the host headers but NOT the native lib.
$(BUILD_DIR)/test_signature_policy_config: host/test_signature_policy_config.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -Ihost $< -o $@

# Internal-plugin registration transaction test — supplies a deterministic HostApi
# table, so it verifies C++ Runtime ownership without a native runtime library.
$(BUILD_DIR)/test_internal_plugin_runtime: host/test_internal_plugin_runtime.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -Ihost $< -o $@

$(BUILD_DIR)/test_reload_runtime: host/test_reload_runtime.cpp | $(BUILD_DIR)
	@if [ -z "$(POLYPLUG_TARGET_DIR)" ]; then \
		echo "FATAL: libpolyplug not found under ../../target/{release,debug} — this runtime test must not silently pass." >&2; \
		echo "Build the core and the native loader first:" >&2; \
		echo "  cargo build --release -p polyplug -p polyplug_native" >&2; \
		exit 1; \
	fi
	@if ! ls $(POLYPLUG_TARGET_DIR)/libpolyplug_native.* >/dev/null 2>&1; then \
		echo "FATAL: libpolyplug_native not found in $(POLYPLUG_TARGET_DIR) — this runtime test must not silently pass." >&2; \
		echo "Build the native loader first:" >&2; \
		echo "  cargo build --release -p polyplug_native" >&2; \
		exit 1; \
	fi
	$(CXX) $(CXXFLAGS) $(INCLUDES) -Ihost -Iloaders/native \
		-DPOLYPLUG_FIXTURES_DIR=\"$(FIXTURES_DIR)\" \
		$< -o $@ \
		-L$(POLYPLUG_TARGET_DIR) -Wl,-rpath,$(abspath $(POLYPLUG_TARGET_DIR)) \
		-lpolyplug -lpolyplug_native

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

clean:
	rm -rf $(BUILD_DIR)

-include $(BUILD_DIR)/*.d
