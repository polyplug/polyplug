// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Base class for guest-side contract implementations.
//
// Plugin authors derive from polyplug::Contract to implement a contract.
// The derived class must provide a static interface() that returns a pointer
// to a statically-allocated GuestContractInterface.
//
// The GuestContractInterface must remain alive for the entire lifetime of the runtime —
// i.e. it should be a static variable.

#pragma once

#include "../../abi/polyplug/abi.hpp"

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Abstract base class for a polyplug contract implementation.
///
/// A contract is a set of functions grouped under a unique contract_id
/// (FNV-1a hash of "contract_name@major_version"). Plugin bundles register
/// one Contract instance per supported contract during polyplug_init.
///
/// Lifetime: Contract instances must outlive the runtime (static allocation
/// or heap allocation that is not freed until after polyplug_runtime_destroy).
///
/// Example:
///   class ImageDecoder final : public polyplug::Contract {
///   public:
///       const GuestContractInterface* interface() const noexcept override {
///           return &kInterface;
///       }
///   private:
///       static const GuestContractInterface kInterface;
///   };
class Contract {
public:
    virtual ~Contract() = default;

    /// Returns a pointer to the statically-allocated GuestContractInterface for this
    /// contract. The returned pointer MUST be valid for the lifetime of the
    /// runtime — never return a pointer to a stack-allocated interface.
    virtual const GuestContractInterface* interface() const noexcept = 0;
};

}  // namespace polyplug