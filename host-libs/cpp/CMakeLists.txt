cmake_minimum_required(VERSION 3.16)
project(polyplug-cpp VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(polyplug_runtime INTERFACE)
target_include_directories(polyplug_runtime INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}
)

add_subdirectory(loaders/native)
add_subdirectory(loaders/python)
add_subdirectory(loaders/lua)
add_subdirectory(loaders/js)
add_subdirectory(loaders/js_deno)
add_subdirectory(loaders/dotnet)