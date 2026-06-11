// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Single-include convenience header for polyplug guest-side C++ integration.
//
// Simply add the sdks/cpp directory to your compiler's include path
// and use:
//
//   #include <polyplug_guest.hpp>
//
// This pulls in all guest-side headers in dependency order:
//   1. abi/polyplug/abi.hpp      — C ABI structs, constants, allocator decls, and StringView helpers
//   2. polyplug/contract.hpp     — Abstract base class for contract implementations
//   3. polyplug/guest.hpp        — alloc_string(host, s) helper + POLYPLUG_GUEST_MAIN
//
// NOTE: the guest SDK holds no process-wide state — the HostApi pointer flows
// through create_instance into the per-instance payload (see polyplug/guest.hpp).

#pragma once

#include "../abi/polyplug/abi.hpp"
#include "polyplug/contract.hpp"
#include "polyplug/guest.hpp"