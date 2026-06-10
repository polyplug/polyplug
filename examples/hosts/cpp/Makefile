CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2 -MMD -MP
INCLUDES = -I../../../sdks/cpp/host \
           -I../../../sdks/cpp/abi \
           -I../../../sdks/cpp/loaders/native \
           -I../../../sdks/cpp/loaders/python \
           -I../../../sdks/cpp/loaders/lua \
           -I../../../sdks/cpp/loaders/js \
           -I../../../sdks/cpp/loaders/dotnet

# Library paths
LIBS_DIR = ../../../target/release/deps
LDFLAGS = -L$(LIBS_DIR) -Wl,-rpath,$(LIBS_DIR)
LIBS = -lpolyplug -lpolyplug_native -lpolyplug_python -lpolyplug_lua -lpolyplug_js -lpolyplug_dotnet

TARGET = host
HOT_RELOAD_TARGET = hot_reload_host
SRC = host.cpp
HOT_RELOAD_SRC = main.cpp

# Compiler-generated dependency files (-MMD -MP) so header changes
# (generated headers, SDK headers, abi.hpp) trigger a rebuild.
DEPFILES = $(TARGET).d $(HOT_RELOAD_TARGET).d

all: $(TARGET) $(HOT_RELOAD_TARGET)

$(TARGET): $(SRC)
	$(CXX) $(CXXFLAGS) -MF $(TARGET).d $(INCLUDES) $(LDFLAGS) -o $@ $< $(LIBS)

$(HOT_RELOAD_TARGET): $(HOT_RELOAD_SRC)
	$(CXX) $(CXXFLAGS) -MF $(HOT_RELOAD_TARGET).d $(INCLUDES) $(LDFLAGS) -o $@ $< $(LIBS)

clean:
	rm -f $(TARGET) $(HOT_RELOAD_TARGET) $(DEPFILES)

-include $(DEPFILES)

.PHONY: all clean
