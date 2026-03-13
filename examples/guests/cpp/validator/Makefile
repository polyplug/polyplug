CXXFLAGS = -std=c++20 -O2 -shared -fPIC -I../../../.. -I../../../../guest-libs/cpp

all: libvalidator.so

libvalidator.so: validator.cpp
	$(CXX) $(CXXFLAGS) -o $@ $^

.PHONY: all
