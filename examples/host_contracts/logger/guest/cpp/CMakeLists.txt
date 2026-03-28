cmake_minimum_required(VERSION 3.10)
project(worker_cpp)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Include directories
include_directories(${CMAKE_SOURCE_DIR}/generated/guest)
include_directories(../../../../../sdks/cpp/guest)

# Source files
set(SOURCES
    plugin.cpp
)

# Build shared library
add_library(worker SHARED ${SOURCES})

# Set output name
set_target_properties(worker PROPERTIES
    PREFIX "lib"
    OUTPUT_NAME "worker"
)
