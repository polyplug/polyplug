// Re-export all types from the auto-generated abi module.
// The auto-generated file is at sdks/js/abi/abi.ts (per D-28).
export * from "./abi.ts";

// Re-export the FFI abstraction seam so `@polyplug/abi` consumers obtain the
// backend through one stable surface (same-package re-export, like ./abi.ts above).
export * from "./ffi/index.ts";
