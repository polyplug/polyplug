# polyplug_loaders_lua - Lua plugin loader package
cmake_minimum_required(VERSION 3.16)

add_library(polyplug_loaders_lua INTERFACE)
target_include_directories(polyplug_loaders_lua INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}/../..
)
target_link_libraries(polyplug_loaders_lua INTERFACE polyplug_runtime)