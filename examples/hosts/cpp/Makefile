CXX = g++
CXXFLAGS = -std=c++17 -Wall -Wextra -O2
INCLUDES = -I../../../host-libs/cpp

# Library paths
LIBS_DIR = ../../../target/release/deps
LDFLAGS = -L$(LIBS_DIR) -Wl,-rpath,$(LIBS_DIR)
LIBS = -lpolyplug -lstdc++fs

TARGET = host
SRC = host.cpp

all: $(TARGET)

$(TARGET): $(SRC)
	$(CXX) $(CXXFLAGS) $(INCLUDES) $(LDFLAGS) -o $@ $< $(LIBS)

clean:
	rm -f $(TARGET)

.PHONY: all clean
