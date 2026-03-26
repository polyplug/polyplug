// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Single-include convenience header for polyplug host-side C++ integration.
//
// Simply add the sdks/cpp directory to your compiler's include path
// and use:
//
//   #include <polyplug.hpp>
//
// This pulls in all host-side headers in dependency order:
//   1. abi/polyplug/abi.hpp     — C ABI structs, constants, and C function decls
//   2. polyplug/handle.hpp      — PluginHandle operator overloads and utilities
//   3. polyplug/error.hpp       — PluginError exception and throw_if_error()
//   4. polyplug/runtime.hpp     — RAII Runtime class with Builder pattern

#pragma once

#include "../abi/polyplug/abi.hpp"
#include "../abi/polyplug/string_view_helper.hpp"
#include "polyplug/handle.hpp"
#include "polyplug/error.hpp"
#include "polyplug/runtime.hpp"