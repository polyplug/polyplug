// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Single-include convenience header for polyplug host-side C++ integration.
//
// Simply add the sdks/cpp directory to your compiler's include path
// and use:
//
//   #include <polyplug.hpp>
//
// This pulls in all host-side headers in dependency order:
//   1. abi/polyplug/abi.hpp     — C ABI structs, constants, C function decls, and StringView helpers
//   2. polyplug/id.hpp          — compile-time bundle/contract ID helpers (FNV-1a 64)
//   3. polyplug/handle.hpp      — GuestContractHandle operator overloads and utilities
//   4. polyplug/error.hpp       — HostException exception and throw_if_error()
//   5. polyplug/runtime.hpp     — RAII Runtime class with Builder pattern

#pragma once

#include "../abi/polyplug/abi.hpp"
#include "polyplug/id.hpp"
#include "polyplug/handle.hpp"
#include "polyplug/error.hpp"
#include "polyplug/runtime.hpp"