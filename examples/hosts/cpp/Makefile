CXX := g++
CXXFLAGS := -std=c++20 -Wall -Wextra -Wpedantic -O2
INCLUDES := -I../../.. -I../../../host-libs/cpp
LDFLAGS := -L../../../examples/hosts/js/target/debug -lpolyplug_full -ldl -pthread -Wl,-rpath,'$$ORIGIN/../../../examples/hosts/js/target/debug'

TARGET := polyplug_host_cpp
SRC := main.cpp

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(SRC)
	$(CXX) $(CXXFLAGS) $(INCLUDES) -o $@ $(SRC) $(LDFLAGS)

clean:
	rm -f $(TARGET)
