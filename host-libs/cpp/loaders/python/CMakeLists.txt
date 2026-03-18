# polyplug_loaders_python - Python plugin loader package
cmake_minimum_required(VERSION 3.16)

add_library(polyplug_loaders_python INTERFACE)
target_include_directories(polyplug_loaders_python INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}/../..
)
target_link_libraries(polyplug_loaders_python INTERFACE polyplug_runtime)