// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Single-include convenience header for polyplug guest-side C++ integration.
//
// Simply add the guest-libs/cpp directory to your compiler's include path
// and use:
//
//   #include <polyplug_guest.hpp>
//
// This pulls in all three guest-side headers in dependency order:
//   1. polyplug/abi.hpp      — C ABI structs, constants, and allocator decls
//   2. polyplug/contract.hpp — Abstract base class for contract implementations
//   3. polyplug/guest.hpp    — operator new/delete overrides + POLYPLUG_GUEST_MAIN
//
// NOTE: polyplug/guest.hpp defines global operator new/delete overloads and
// must be included in exactly one translation unit per plugin DSO.

#pragma once

#include <polyplug/abi.hpp>
#include <polyplug/contract.hpp>
#include <polyplug/guest.hpp>
