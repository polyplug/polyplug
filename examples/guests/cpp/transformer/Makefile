CXXFLAGS = -std=c++20 -O2 -shared -fPIC -I../../../.. -I../../../../guest-libs/cpp

all: libuppercase_transformer.so

libuppercase_transformer.so: uppercase_transformer.cpp
	$(CXX) $(CXXFLAGS) -o $@ $^

.PHONY: all
