//! Extensions — Extension trait and SendPtr helper for the extension system.

pub mod trace;

/// A raw pointer wrapper that implements Send and Sync.
///
/// `*const ()` is not `Send` by default. This newtype wraps it so it can be
/// stored in `OnceLock<HashMap<u32, SendPtr>>` on the global extension map.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SendPtr(pub *const ());

// SAFETY: SendPtr wraps a raw pointer to a 'static extension vtable.
// Extension vtables are written once during RuntimeBuilder::build() and never mutated.
// All accesses are read-only after initialization. The pointed-to data outlives any
// thread that reads this pointer (vtable lifetime is Runtime lifetime).
unsafe impl Send for SendPtr {}
// SAFETY: Same reasoning as Send — concurrent reads of a static vtable pointer are safe.
unsafe impl Sync for SendPtr {}

/// Trait implemented by all host-side extension types.
///
/// Extensions provide optional vtables that guest plugins can query at init time
/// via `host_get_extension`. The vtable pointer MUST remain valid for the entire
/// lifetime of the `Runtime`.
pub trait Extension: Send + Sync {
    /// Returns the FNV-1a 32-bit extension ID (e.g. fnv1a_32(b"trace")).
    fn extension_id(&self) -> u32;
    /// Returns a raw pointer to the extension's C-ABI vtable struct.
    /// The pointer MUST remain valid for the entire lifetime of the Runtime.
    fn vtable_ptr(&self) -> *const ();
}
