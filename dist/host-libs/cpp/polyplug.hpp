// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Single-include convenience header for polyplug host-side C++ integration.
//
// Simply add the polyplug include directory to your compiler's include path
// and use:
//
//   #include <polyplug.hpp>
//
// This pulls in all four host-side headers in dependency order:
//   1. polyplug/abi.hpp     — C ABI structs, constants, and C function decls
//   2. polyplug/handle.hpp  — PluginHandle operator overloads and utilities
//   3. polyplug/error.hpp   — PluginError exception and throw_if_error()
//   4. polyplug/runtime.hpp — RAII Runtime class with Builder pattern

#pragma once

#include <polyplug/abi.hpp>
#include <polyplug/handle.hpp>
#include <polyplug/error.hpp>
#include <polyplug/runtime.hpp>
