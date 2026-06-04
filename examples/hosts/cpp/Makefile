CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2
INCLUDES = -I../../../sdks/cpp/host \
           -I../../../sdks/cpp/abi \
           -I../../../sdks/cpp/loaders/native \
           -I../../../sdks/cpp/loaders/python \
           -I../../../sdks/cpp/loaders/lua \
           -I../../../sdks/cpp/loaders/js

# Library paths
LIBS_DIR = ../../../target/release/deps
LDFLAGS = -L$(LIBS_DIR) -Wl,-rpath,$(LIBS_DIR)
LIBS = -lpolyplug -lpolyplug_native -lpolyplug_python -lpolyplug_lua -lpolyplug_js

TARGET = host
SRC = host.cpp

all: $(TARGET)

$(TARGET): $(SRC)
	$(CXX) $(CXXFLAGS) $(INCLUDES) $(LDFLAGS) -o $@ $< $(LIBS)

clean:
	rm -f $(TARGET)

.PHONY: all clean
