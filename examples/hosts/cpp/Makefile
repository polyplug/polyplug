CXX      := g++
CXXFLAGS := -std=c++20 -Wall -Wextra -Wpedantic -O2
INCLUDES := -I../../.. -I../../../host-libs/cpp
LDFLAGS  := -L../../../target/debug \
            -lpolyplug -lpolyplug_native -lpolyplug_dotnet \
            -lpolyplug_python -lpolyplug_lua -lpolyplug_js \
            -ldl -pthread \
            -Wl,-rpath,'$$ORIGIN/../../../target/debug'

PIPELINE_TARGET := polyplug_host_cpp
PIPELINE_SRC    := main.cpp

HOST_TARGET := host
HOST_SRC    := host.cpp

.PHONY: all host clean

all: $(PIPELINE_TARGET) $(HOST_TARGET)

$(PIPELINE_TARGET): $(PIPELINE_SRC)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -o $@ $< $(LDFLAGS)

$(HOST_TARGET): $(HOST_SRC)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -o $@ $< $(LDFLAGS)

clean:
	rm -f $(PIPELINE_TARGET) $(HOST_TARGET)
