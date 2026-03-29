cmake_minimum_required(VERSION 3.16)
project(logger_host)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_executable(logger_host main.cpp)

target_include_directories(logger_host PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../../../../sdks/cpp/host
    ${CMAKE_CURRENT_SOURCE_DIR}/../../../../../sdks/cpp/abi
    ${CMAKE_CURRENT_SOURCE_DIR}/../../../../../sdks/cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/generated/host
)

target_link_libraries(logger_host PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../../../../target/release/deps/libpolyplug.so
)