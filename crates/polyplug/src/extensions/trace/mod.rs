//! Trace extension — TraceVTable and TraceExtension for host-side tracing.

use crate::abi::StringView;
use crate::extensions::Extension;

/// FNV-1a 32-bit hash of b"trace". Verified by unit test.
pub const EXT_TRACE_ID: u32 = 0xC4EB9AEE_u32;

/// C-ABI vtable for the trace extension. Passed to plugins as a *const TraceVTable.
#[repr(C)]
pub struct TraceVTable {
    /// Emit a trace message. msg is valid UTF-8 for call duration.
    /// state is the opaque TraceState pointer (same as this vtable's state field).
    pub emit: unsafe extern "C" fn(msg: StringView, state: *const ()),
    /// Opaque pointer to the heap-allocated TraceState (leaked, never freed).
    pub state: *const (),
}

// SAFETY: TraceVTable fields are a function pointer and a *const () to a leaked allocation.
// Function pointers are thread-safe. The state pointer is immutable after construction.
unsafe impl Send for TraceVTable {}
// SAFETY: Same reasoning — no mutable state, concurrent reads are safe.
unsafe impl Sync for TraceVTable {}

/// Internal state carrying the user callback. Stored as a leaked Box.
struct TraceState {
    callback: Box<dyn Fn(&str) + Send + Sync + 'static>,
}

/// C-ABI thunk: calls the Rust closure stored in TraceState.
///
/// # Safety
/// `state` must be a non-null pointer to a `TraceState` created in `TraceExtension::new`.
/// `msg.ptr` must point to valid UTF-8 bytes for `msg.len` bytes.
unsafe extern "C" fn trace_emit_thunk(msg: StringView, state: *const ()) {
    // SAFETY: state is a non-null *const TraceState leaked in TraceExtension::new.
    // The TraceState outlives any call through this vtable (it is never freed).
    let ts: *const TraceState = state as *const TraceState;
    // SAFETY: ABI contract guarantees msg.ptr points to valid UTF-8 for msg.len bytes,
    // and remains valid for the duration of this call.
    let s: &str =
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(msg.ptr, msg.len)) };
    // SAFETY: ts is non-null and properly aligned (guaranteed by Box::into_raw).
    unsafe { ((*ts).callback)(s) };
}

/// Host-side trace extension. Wraps a callback and exposes it via a C-ABI vtable.
pub struct TraceExtension {
    /// Leaked TraceVTable — stable pointer for the lifetime of the Runtime.
    vtable: *const TraceVTable,
}

// SAFETY: TraceExtension holds only a pointer to a leaked (never-freed) TraceVTable.
// The callback inside is Send + Sync. The vtable pointer is valid until process exit.
unsafe impl Send for TraceExtension {}
// SAFETY: Same — no mutable state; vtable pointer is read-only after construction.
unsafe impl Sync for TraceExtension {}

impl TraceExtension {
    /// Create a new `TraceExtension` from a callback.
    ///
    /// The callback is called whenever a plugin emits a trace message.
    /// The callback must be `Send + Sync + 'static` so it can be safely called
    /// from any thread through the C ABI.
    pub fn new(callback: impl Fn(&str) + Send + Sync + 'static) -> TraceExtension {
        let state: Box<TraceState> = Box::new(TraceState {
            callback: Box::new(callback),
        });
        let state_ptr: *const TraceState = Box::into_raw(state);
        let vtable: Box<TraceVTable> = Box::new(TraceVTable {
            emit: trace_emit_thunk,
            state: state_ptr as *const (),
        });
        let vtable_ptr: *const TraceVTable = Box::into_raw(vtable);
        TraceExtension { vtable: vtable_ptr }
    }
}

impl Extension for TraceExtension {
    fn extension_id(&self) -> u32 {
        EXT_TRACE_ID
    }

    fn vtable_ptr(&self) -> *const () {
        self.vtable as *const ()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_trace_id_matches_runtime_hash() {
        assert_eq!(
            TraceExtension::new(|_| {}).extension_id(),
            crate::abi::extension_id("trace")
        );
    }
}
