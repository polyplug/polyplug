CXXFLAGS = -std=c++20 -O2 -shared -fPIC -I../../../.. -I../../../../guest-libs/cpp

all: libencoder.so

libencoder.so: encoder.cpp
	$(CXX) $(CXXFLAGS) -o $@ $^

.PHONY: all
