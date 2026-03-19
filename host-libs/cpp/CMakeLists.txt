cmake_minimum_required(VERSION 3.16)
project(polyplug-cpp VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Add cmake module path for FindPolyplug.cmake
list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

# Find polyplug native library (optional for header-only usage, required for runtime)
# Set POLYPLUG_LIB environment variable or POLYPLUG_DOWNLOAD=ON to enable download
find_package(Polyplug QUIET)

add_library(polyplug_runtime INTERFACE)
target_include_directories(polyplug_runtime INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}
)

# Link against polyplug native library if found
if(Polyplug_FOUND)
    target_link_libraries(polyplug_runtime INTERFACE Polyplug::polyplug)
    message(STATUS "Polyplug native library found: ${Polyplug_LIBRARY}")
else()
    message(STATUS "Polyplug native library not found. Set POLYPLUG_LIB or POLYPLUG_DOWNLOAD=ON")
endif()

add_subdirectory(loaders/native)
add_subdirectory(loaders/python)
add_subdirectory(loaders/lua)
add_subdirectory(loaders/js)
add_subdirectory(loaders/js_deno)
add_subdirectory(loaders/dotnet)