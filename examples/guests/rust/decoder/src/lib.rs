use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineDecoderGuestContract;

/// The decode transformation itself, factored so the registered `decode` contract
/// AND the raw-FFI baseline export (`polyplug_bench_decode`) run byte-identical
/// work — the only thing that differs between them is the calling mechanism
/// (polyplug dispatch vs a hand-rolled `dlsym` call), which is exactly what the
/// cross-language matrix baseline isolates.
#[inline]
fn decode_body(input: &str) -> String {
    format!("DECODED:{}", input.replace(',', "|"))
}

struct Plugin {
    /// Host handle for this runtime, captured at instance creation.
    host: HostContext,
}

impl PipelineDecoderGuestContract for Plugin {
    fn decode(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView whose bytes stay live for the
        // duration of this call, per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) }?;
        self.host.alloc_string(&decode_body(s))
    }
}

/// Raw-FFI baseline export: the SAME `decode_body` the registered `decode` contract
/// runs, reached by `dlsym` instead of polyplug dispatch. Allocates the result with
/// the Rust global allocator (a plain plugin author's `malloc`-equivalent) and
/// returns it through out-params; the caller reads it and calls
/// `polyplug_bench_decode_free`. This is the "what you'd hand-write WITHOUT polyplug"
/// floor that the cross-language matrix is measured against.
///
/// # Safety
/// `in_ptr` / `in_len` must describe a valid UTF-8 byte range; `out_ptr` / `out_len`
/// must be non-null, writable out-pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_bench_decode(
    in_ptr: *const u8,
    in_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) {
    // SAFETY: the caller guarantees in_ptr/in_len describe a valid byte range.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(in_ptr, in_len) };
    let result: String = match core::str::from_utf8(bytes) {
        Ok(input) => decode_body(input),
        Err(_) => String::new(),
    };
    let mut boxed: Box<[u8]> = result.into_bytes().into_boxed_slice();
    let len: usize = boxed.len();
    let ptr: *mut u8 = boxed.as_mut_ptr();
    core::mem::forget(boxed);
    // SAFETY: out_ptr / out_len are non-null, writable out-pointers per the contract.
    unsafe {
        *out_ptr = ptr;
        *out_len = len;
    }
}

/// Free a buffer returned by [`polyplug_bench_decode`].
///
/// # Safety
/// `ptr` / `len` must be exactly the pair a prior `polyplug_bench_decode` call wrote.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_bench_decode_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: reconstruct the exact `Box<[u8]>` that polyplug_bench_decode forgot.
    // `slice_from_raw_parts_mut` builds the `*mut [u8]` fat pointer directly — no
    // intermediate `&mut [u8]` reference — for `Box::from_raw` to take ownership of.
    unsafe {
        drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr, len)));
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_decoder(host: HostContext) -> Box<dyn PipelineDecoderGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}
