# Test runner for the hand-written C++ SDK test programs.
#
# - abi/test_layout.cpp is a compile-time suite (static_assert only, no main):
#   compiling it IS the test — every layout assert is evaluated by the compiler.
# - guest/test_alloc_tracking.cpp is a runtime test with its own main and
#   non-zero exit on failure.
#
# Mirrors examples/hosts/cpp's depfile pattern (-MMD -MP) so header changes
# (abi.hpp, guest.hpp) trigger rebuilds.
#
# Run from the repo root with: make -C sdks/cpp test   (or: just test-sdk-cpp)

CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2 -MMD -MP
INCLUDES = -Iabi -Iguest
BUILD_DIR = build

.PHONY: test clean

test: $(BUILD_DIR)/test_layout.o $(BUILD_DIR)/test_alloc_tracking
	@echo "OK: abi/test_layout.cpp compiled - all ABI layout static_asserts hold"
	$(BUILD_DIR)/test_alloc_tracking

$(BUILD_DIR)/test_layout.o: abi/test_layout.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -c $< -o $@

$(BUILD_DIR)/test_alloc_tracking: guest/test_alloc_tracking.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $(INCLUDES) $< -o $@

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

clean:
	rm -rf $(BUILD_DIR)

-include $(BUILD_DIR)/*.d
