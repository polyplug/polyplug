# FindPolyplug.cmake
# Find the polyplug native library and provide include paths
#
# This module defines:
#   Polyplug_FOUND        - True if polyplug library was found
#   Polyplug_LIBRARY      - Path to the polyplug shared library
#   Polyplug_INCLUDE_DIR  - Path to the polyplug headers
#   Polyplug_LIBRARIES    - Same as Polyplug_LIBRARY (for convenience)
#   Polyplug_INCLUDE_DIRS - Same as Polyplug_INCLUDE_DIR (for convenience)
#
# Configuration options:
#   POLYPLUG_LIB          - Environment variable or CMake variable specifying library path
#   POLYPLUG_DOWNLOAD     - If ON, download library from GitHub Releases if not found locally
#   POLYPLUG_VERSION      - Version to download (default: "0.1.0")
#
# Search order:
#   1. POLYPLUG_LIB environment variable
#   2. _native/{platform}-{arch}/ relative to CMAKE_CURRENT_SOURCE_DIR
#   3. System library paths
#   4. GitHub Releases download (if POLYPLUG_DOWNLOAD is ON)

cmake_minimum_required(VERSION 3.16)

# Detect platform identifier
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(PLATFORM_IDENTIFIER "linux-x64")
    set(LIBRARY_SUFFIX ".so")
elseif(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    if(CMAKE_SYSTEM_PROCESSOR STREQUAL "arm64")
        set(PLATFORM_IDENTIFIER "darwin-arm64")
    else()
        set(PLATFORM_IDENTIFIER "darwin-x64")
    endif()
    set(LIBRARY_SUFFIX ".dylib")
elseif(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(PLATFORM_IDENTIFIER "win32-x64")
    set(LIBRARY_SUFFIX ".dll")
else()
    message(FATAL_ERROR "Unsupported platform: ${CMAKE_SYSTEM_NAME}")
endif()

# Default version for download
set(POLYPLUG_VERSION "0.1.0" CACHE STRING "Polyplug version to download from GitHub Releases")

# Determine library name based on platform
if(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(LIBRARY_NAME "polyplug-windows-x64${LIBRARY_SUFFIX}")
else()
    set(LIBRARY_NAME "libpolyplug${LIBRARY_SUFFIX}")
endif()

# Search paths
set(_POLYPLUG_SEARCH_PATHS
    ENV POLYPLUG_LIB
    ${CMAKE_CURRENT_SOURCE_DIR}/_native/${PLATFORM_IDENTIFIER}
    ${CMAKE_SOURCE_DIR}/_native/${PLATFORM_IDENTIFIER}
    /usr/local/lib
    /usr/lib
    /opt/local/lib
)

# Add environment variable path explicitly
if(ENV{POLYPLUG_LIB})
    list(INSERT _POLYPLUG_SEARCH_PATHS 0 $ENV{POLYPLUG_LIB})
endif()

# Find the library
find_library(Polyplug_LIBRARY
    NAMES polyplug libpolyplug ${LIBRARY_NAME}
    PATHS ${_POLYPLUG_SEARCH_PATHS}
    PATH_SUFFIXES lib lib64
    DOC "Path to the polyplug shared library"
)

# Find include directory (headers are in host-libs/cpp/)
find_path(Polyplug_INCLUDE_DIR
    NAMES polyplug.hpp
    PATHS
        ${CMAKE_CURRENT_SOURCE_DIR}/..
        ${CMAKE_SOURCE_DIR}/host-libs/cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/host-libs/cpp
        ENV POLYPLUG_INCLUDE
    DOC "Path to the polyplug C++ headers"
)

# Handle download option
if(POLYPLUG_DOWNLOAD AND NOT Polyplug_LIBRARY)
    message(STATUS "Polyplug library not found locally. Downloading from GitHub Releases...")
    
    set(DOWNLOAD_URL "https://github.com/polyplug/polyplug/releases/download/v${POLYPLUG_VERSION}/${LIBRARY_NAME}")
    set(DOWNLOAD_DIR "${CMAKE_CURRENT_SOURCE_DIR}/_native/${PLATFORM_IDENTIFIER}")
    set(DOWNLOAD_PATH "${DOWNLOAD_DIR}/${LIBRARY_NAME}")
    
    # Create directory if it doesn't exist
    file(MAKE_DIRECTORY ${DOWNLOAD_DIR})
    
    # Download the library
    file(DOWNLOAD
        ${DOWNLOAD_URL}
        ${DOWNLOAD_PATH}
        SHOW_PROGRESS
        STATUS DOWNLOAD_STATUS
        EXPECTED_HASH SHA256=0
    )
    
    list(GET DOWNLOAD_STATUS 0 STATUS_CODE)
    list(GET DOWNLOAD_STATUS 1 STATUS_MESSAGE)
    
    if(STATUS_CODE EQUAL 0)
        message(STATUS "Successfully downloaded polyplug library to ${DOWNLOAD_PATH}")
        set(Polyplug_LIBRARY ${DOWNLOAD_PATH})
    else()
        message(WARNING "Failed to download polyplug library: ${STATUS_MESSAGE}")
        message(WARNING "Download URL: ${DOWNLOAD_URL}")
        message(WARNING "Please set POLYPLUG_LIB environment variable or install polyplug manually")
    endif()
endif()

# Handle standard find_package arguments
include(FindPackageHandleStandardArgs)
find_package_handle_standard_args(Polyplug
    REQUIRED_VARS Polyplug_LIBRARY Polyplug_INCLUDE_DIR
    VERSION_VAR POLYPLUG_VERSION
)

# Set convenience variables
if(Polyplug_FOUND)
    set(Polyplug_LIBRARIES ${Polyplug_LIBRARY})
    set(Polyplug_INCLUDE_DIRS ${Polyplug_INCLUDE_DIR})
    
    # Create imported target for easier usage
    if(NOT TARGET Polyplug::polyplug)
        add_library(Polyplug::polyplug SHARED IMPORTED)
        set_target_properties(Polyplug::polyplug PROPERTIES
            IMPORTED_LOCATION ${Polyplug_LIBRARY}
            INTERFACE_INCLUDE_DIRECTORIES ${Polyplug_INCLUDE_DIR}
        )
    endif()
endif()

# Mark variables as advanced
mark_as_advanced(
    Polyplug_LIBRARY
    Polyplug_INCLUDE_DIR
    POLYPLUG_VERSION
)
