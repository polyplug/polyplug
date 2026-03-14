CXXFLAGS = -std=c++20 -O2 -shared -fPIC -I../../../.. -I../../../../guest-libs/cpp

all: libreporter.so

libreporter.so: reporter.cpp
	$(CXX) $(CXXFLAGS) -o $@ $^

.PHONY: all
