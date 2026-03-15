//! polyplug — core plugin runtime for the polyplug platform.
//!
//! This crate exports:
//! - 16 `#[no_mangle]` C ABI entry points (see `ffi`, `abi`, and `allocator` modules)
//! - Module-level access to all subsystems

pub mod abi;
pub mod allocator;
pub mod error;
pub mod extensions;
pub mod graph;
pub mod loader;
pub mod registry;
pub mod reload;
pub mod runtime;
pub use reload::ReloadEvent;

pub mod ffi;
pub mod version;

// Re-export the allocator functions at crate level for convenience.
// These are also exported with #[no_mangle] from allocator/mod.rs.
pub use allocator::polyplug_host_alloc;
pub use allocator::polyplug_host_free;
