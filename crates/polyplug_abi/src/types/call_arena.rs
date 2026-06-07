//! Call arena — a per-call bump allocator for variable-size return values.
//!
//! A `CallArena` is a stack-backed bump allocator that the host hands to a VM
//! dispatch call so the guest can write variable-size outputs (strings, buffers)
//! into host-controlled memory without a `host->alloc` round trip per value.
//!
//! # Ownership and lifetime
//!
//! - The arena is **constructor-owned**: it borrows a caller-provided byte buffer
//!   (typically an inline `[u8; N]` field on the generated host caller) and a
//!   `HostApi` pointer for overflow allocation. The caller owns both.
//! - Allocations served from the arena are valid **until the next `reset()`** on
//!   the same arena. The generated host caller resets at the start of every call,
//!   so a returned view is valid until the next call on that caller.
//! - Guests **never free** arena allocations. The arena owns every overflow block
//!   and frees them all on `reset()` (and the caller must `reset()` to release the
//!   final batch before dropping the arena).

use crate::host::HostApi;

/// Header prepended to every host-allocated overflow block.
///
/// Overflow blocks form a singly linked list rooted at `CallArena.first_overflow`.
/// Each block stores the total `capacity` it was allocated with (including this
/// header) so `reset()` can free it with the exact size/align the host expects.
#[repr(C)]
#[derive(Debug)]
pub struct ArenaOverflowBlock {
    /// Next overflow block in the chain, or null for the last block.
    pub next: *mut ArenaOverflowBlock,
    /// Total allocated size of this block in bytes, including this header.
    pub capacity: usize,
}

/// Per-call bump allocator handed to a VM dispatch call.
///
/// # Layout
///
/// `#[repr(C)]` with five pointer-sized fields (40 bytes, align 8). The first
/// three fields define the primary bump region `[base, end)` with `cur` as the
/// next free byte. When the primary region is exhausted, `alloc` requests a fresh
/// block from `host->alloc`, chains it onto `first_overflow`, and serves from it.
///
/// A null `CallArena*` passed to a dispatch function means "no arena": the bridge
/// falls back to per-value `host->alloc`.
#[repr(C)]
#[derive(Debug)]
pub struct CallArena {
    /// Next free byte in the primary region.
    pub cur: *mut u8,
    /// One past the last usable byte of the primary region.
    pub end: *mut u8,
    /// Start of the primary region (the reset target for `cur`).
    pub base: *mut u8,
    /// Host API used to allocate and free overflow blocks.
    pub host: *const HostApi,
    /// Head of the singly linked list of host-allocated overflow blocks.
    pub first_overflow: *mut ArenaOverflowBlock,
}

/// Minimum size of a host-allocated overflow block, including its header.
const OVERFLOW_BLOCK_MIN: usize = 4096;

/// Alignment used for host-allocated overflow blocks.
///
/// Blocks are allocated with the maximum alignment any served value needs.
/// `align_of::<ArenaOverflowBlock>()` (pointer alignment) covers the header and
/// every primitive ABI value; larger requested alignments are satisfied by the
/// bump-up logic within the block.
const OVERFLOW_BLOCK_ALIGN: usize = core::mem::align_of::<ArenaOverflowBlock>();

impl CallArena {
    /// Construct an arena over a caller-provided byte buffer.
    ///
    /// `buf` defines the primary bump region; `host` is used only for overflow
    /// allocation (and may be null only if the caller guarantees no overflow,
    /// in which case overflow requests fail and `alloc` returns null).
    ///
    /// The returned arena borrows `buf` for its lifetime.
    pub fn new(buf: &mut [u8], host: *const HostApi) -> CallArena {
        let base: *mut u8 = buf.as_mut_ptr();
        // SAFETY: `base` and `base.add(buf.len())` are the start and one-past-end
        // of the same allocation, so the offset is in bounds for pointer arithmetic.
        let end: *mut u8 = unsafe { base.add(buf.len()) };
        CallArena {
            cur: base,
            end,
            base,
            host,
            first_overflow: core::ptr::null_mut(),
        }
    }

    /// Allocate `size` bytes aligned to `align` from the arena.
    ///
    /// Serves from the primary region by bumping `cur`; on exhaustion, requests a
    /// fresh overflow block from the host (at least `OVERFLOW_BLOCK_MIN` bytes,
    /// or large enough for `size + header`) and serves from it. Returns null if
    /// `size == 0`, if `align` is not a power of two, or if the host allocation
    /// fails.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid until the next [`CallArena::reset`].
    pub fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        if size == 0 || !align.is_power_of_two() {
            return core::ptr::null_mut();
        }

        if let Some(ptr) = Self::bump(self.cur, self.end, size, align) {
            self.cur = ptr.wrapping_add(size);
            return ptr;
        }

        self.alloc_overflow(size, align)
    }

    /// Attempt to bump-allocate within `[from, end)`.
    ///
    /// Returns the aligned start pointer if `size` bytes fit, else `None`.
    fn bump(from: *mut u8, end: *mut u8, size: usize, align: usize) -> Option<*mut u8> {
        let addr: usize = from as usize;
        let aligned: usize = addr.checked_add(align - 1)? & !(align - 1);
        let new_cur: usize = aligned.checked_add(size)?;
        if new_cur <= end as usize {
            Some(aligned as *mut u8)
        } else {
            None
        }
    }

    /// Allocate a fresh overflow block from the host and serve `size` from it.
    fn alloc_overflow(&mut self, size: usize, align: usize) -> *mut u8 {
        if self.host.is_null() {
            return core::ptr::null_mut();
        }

        let header: usize = core::mem::size_of::<ArenaOverflowBlock>();
        let needed: usize = match header.checked_add(align).and_then(|v| v.checked_add(size)) {
            Some(v) => v,
            None => return core::ptr::null_mut(),
        };
        let capacity: usize = needed.max(OVERFLOW_BLOCK_MIN);

        // SAFETY: `self.host` is non-null (checked above) and points to a valid
        // HostApi for the arena's lifetime. The allocator returns a block of
        // `capacity` bytes aligned to `OVERFLOW_BLOCK_ALIGN`, or null on failure.
        let block_ptr: *mut u8 =
            unsafe { ((*self.host).alloc)(self.host, capacity, OVERFLOW_BLOCK_ALIGN) };
        if block_ptr.is_null() {
            return core::ptr::null_mut();
        }

        let block: *mut ArenaOverflowBlock = block_ptr.cast::<ArenaOverflowBlock>();
        // SAFETY: `block_ptr` is non-null, aligned for ArenaOverflowBlock, and
        // owns at least `capacity >= header` bytes, so writing the header is sound.
        unsafe {
            block.write(ArenaOverflowBlock {
                next: self.first_overflow,
                capacity,
            });
        }
        self.first_overflow = block;

        // SAFETY: the data region begins just past the header and stays within the
        // `capacity`-byte block; `block_end` is one-past-end of the same block.
        let (data_start, block_end): (*mut u8, *mut u8) =
            unsafe { (block_ptr.add(header), block_ptr.add(capacity)) };
        match Self::bump(data_start, block_end, size, align) {
            Some(ptr) => ptr,
            // `capacity >= header + align + size` guarantees the bump fits.
            None => core::ptr::null_mut(),
        }
    }

    /// Reset the arena, freeing every overflow block and rewinding the primary
    /// region.
    ///
    /// After `reset`, all pointers previously returned by [`CallArena::alloc`] are
    /// invalid. The primary region is reusable for the next call.
    pub fn reset(&mut self) {
        let mut block: *mut ArenaOverflowBlock = self.first_overflow;
        while !block.is_null() {
            // SAFETY: every block in the chain was allocated by `alloc_overflow`
            // with a valid header; reading `next`/`capacity` before freeing is sound.
            let (next, capacity) = unsafe { ((*block).next, (*block).capacity) };
            if !self.host.is_null() {
                // SAFETY: `block` was allocated by `host->alloc` with `capacity`
                // bytes and `OVERFLOW_BLOCK_ALIGN`; freeing with the same args is
                // the required contract.
                unsafe {
                    ((*self.host).free)(
                        self.host,
                        block.cast::<u8>(),
                        capacity,
                        OVERFLOW_BLOCK_ALIGN,
                    );
                }
            }
            block = next;
        }
        self.first_overflow = core::ptr::null_mut();
        self.cur = self.base;
    }
}

// SAFETY: CallArena holds raw pointers into a caller-owned buffer and HostApi.
// It is used single-threaded for the duration of one dispatch call (the loader
// holds its VM lock across the call), so it carries no shared-state hazard beyond
// the pointers it borrows, whose validity the caller guarantees.
unsafe impl Send for CallArena {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::cell::Cell;
    use core::mem::{align_of, offset_of, size_of};

    use super::*;
    use crate::guest::{GuestContractInstance, GuestContractInterface};
    use crate::host::{HostContractInstance, HostContractInterface};
    use crate::plugin::{GuestContractHandle, PluginDescriptor};
    use crate::types::{AbiError, Array, DependencyInfo, StringView};
    use polyplug_utils::BundleId;

    // ─── Counting host allocator for overflow tests ───────────────────────────

    thread_local! {
        static ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
        static FREE_COUNT: Cell<u64> = const { Cell::new(0) };
    }

    unsafe extern "C" fn test_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
        ALLOC_COUNT.with(|c| c.set(c.get() + 1));
        let layout: core::alloc::Layout =
            core::alloc::Layout::from_size_align(size, align).expect("valid layout");
        // SAFETY: layout has non-zero size in every test that triggers overflow.
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe extern "C" fn test_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
        FREE_COUNT.with(|c| c.set(c.get() + 1));
        let layout: core::alloc::Layout =
            core::alloc::Layout::from_size_align(size, align).expect("valid layout");
        // SAFETY: ptr/size/align match the allocation made by test_alloc.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    // CallArena only ever calls `alloc` and `free`. The remaining HostApi fields
    // are populated with panicking stubs so the struct is a fully valid HostApi
    // (no zeroed function pointers) while making any unexpected call obvious.
    unsafe extern "C" fn stub_register_guest(
        _this: *const HostApi,
        _descriptor: *const PluginDescriptor,
        _interface: *const GuestContractInterface,
    ) -> AbiError {
        AbiError::ok()
    }

    unsafe extern "C" fn stub_find(
        _this: *const HostApi,
        _id: u64,
        _ver: u32,
    ) -> GuestContractHandle {
        GuestContractHandle::null()
    }

    unsafe extern "C" fn stub_find_all(
        _this: *const HostApi,
        _id: u64,
        _ver: u32,
    ) -> Array<GuestContractHandle> {
        Array::empty()
    }

    unsafe extern "C" fn stub_resolve_guest(
        _this: *const HostApi,
        _handle: GuestContractHandle,
    ) -> *const GuestContractInterface {
        core::ptr::null()
    }

    unsafe extern "C" fn stub_get_host_contract(
        _this: *const HostApi,
        _id: u64,
        _ver: u32,
    ) -> HostContractInstance {
        HostContractInstance::null()
    }

    unsafe extern "C" fn stub_resolve_host_interface(
        _this: *const HostApi,
        _id: u64,
        _ver: u32,
    ) -> *const HostContractInterface {
        core::ptr::null()
    }

    unsafe extern "C" fn stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
        Array::empty()
    }

    unsafe extern "C" fn stub_get_deps(_this: *const HostApi) -> Array<DependencyInfo> {
        Array::empty()
    }

    unsafe extern "C" fn stub_load(_this: *const HostApi, _p: *const u8, _l: usize) -> AbiError {
        AbiError::ok()
    }

    unsafe extern "C" fn stub_register_host(
        _this: *const HostApi,
        _interface: *const HostContractInterface,
    ) -> AbiError {
        AbiError::ok()
    }

    unsafe extern "C" fn stub_register_loader(
        _this: *const HostApi,
        _name: StringView,
        _loader: *mut core::ffi::c_void,
    ) -> AbiError {
        AbiError::ok()
    }

    unsafe extern "C" fn stub_get_last_error(
        _this: *const HostApi,
        _buf: *mut u8,
        _len: usize,
    ) -> usize {
        0
    }

    unsafe extern "C" fn stub_get_len(_this: *const HostApi) -> usize {
        0
    }

    unsafe extern "C" fn stub_call_guest_method(
        _this: *const HostApi,
        _instance: GuestContractInstance,
        _fn_id: u32,
        _args: *const core::ffi::c_void,
        _out: *mut core::ffi::c_void,
        _arena: *mut CallArena,
    ) -> AbiError {
        AbiError::ok()
    }

    unsafe extern "C" fn stub_get_extension(_this: *const HostApi, _id: u32) -> *const () {
        core::ptr::null()
    }

    unsafe extern "C" fn stub_unload(_this: *const HostApi, _bundle_id: BundleId) -> AbiError {
        AbiError::ok()
    }

    fn test_host() -> HostApi {
        HostApi {
            runtime: core::ptr::null_mut(),
            register_guest_contract: stub_register_guest,
            alloc: test_alloc,
            free: test_free,
            find_guest_contract: stub_find,
            find_all_guest_contracts: stub_find_all,
            resolve_guest_contract: stub_resolve_guest,
            get_host_contract: stub_get_host_contract,
            resolve_host_contract_interface: stub_resolve_host_interface,
            list_bundles: stub_list_bundles,
            get_dependencies: stub_get_deps,
            load_bundle: stub_load,
            reload_bundle: stub_load,
            register_host_contract: stub_register_host,
            register_loader: stub_register_loader,
            get_last_error: stub_get_last_error,
            get_error_len: stub_get_len,
            call_guest_method: stub_call_guest_method,
            get_extension: stub_get_extension,
            unload_bundle: stub_unload,
        }
    }

    fn reset_counters() {
        ALLOC_COUNT.with(|c| c.set(0));
        FREE_COUNT.with(|c| c.set(0));
    }

    #[test]
    fn layout_call_arena() {
        assert_eq!(size_of::<CallArena>(), 40);
        assert_eq!(align_of::<CallArena>(), 8);
        assert_eq!(offset_of!(CallArena, cur), 0);
        assert_eq!(offset_of!(CallArena, end), 8);
        assert_eq!(offset_of!(CallArena, base), 16);
        assert_eq!(offset_of!(CallArena, host), 24);
        assert_eq!(offset_of!(CallArena, first_overflow), 32);

        assert_eq!(size_of::<ArenaOverflowBlock>(), 16);
        assert_eq!(align_of::<ArenaOverflowBlock>(), 8);
        assert_eq!(offset_of!(ArenaOverflowBlock, next), 0);
        assert_eq!(offset_of!(ArenaOverflowBlock, capacity), 8);
    }

    #[test]
    fn bump_serves_from_primary_region() {
        let mut buf: [u8; 64] = [0; 64];
        let mut arena: CallArena = CallArena::new(&mut buf, core::ptr::null());

        let a: *mut u8 = arena.alloc(8, 1);
        let b: *mut u8 = arena.alloc(8, 1);
        assert!(!a.is_null());
        assert!(!b.is_null());
        assert_eq!(b as usize - a as usize, 8);
        assert!(arena.first_overflow.is_null());
    }

    #[test]
    fn alloc_respects_alignment() {
        let mut buf: [u8; 128] = [0; 128];
        let mut arena: CallArena = CallArena::new(&mut buf, core::ptr::null());

        // Burn one byte so the next allocation must round up.
        let _one: *mut u8 = arena.alloc(1, 1);
        let aligned: *mut u8 = arena.alloc(8, 16);
        assert!(!aligned.is_null());
        assert_eq!(aligned as usize % 16, 0);
    }

    #[test]
    fn zero_size_returns_null() {
        let mut buf: [u8; 16] = [0; 16];
        let mut arena: CallArena = CallArena::new(&mut buf, core::ptr::null());
        assert!(arena.alloc(0, 1).is_null());
    }

    #[test]
    fn overflow_chains_blocks() {
        reset_counters();
        let host: HostApi = test_host();
        let mut buf: [u8; 16] = [0; 16];
        let mut arena: CallArena = CallArena::new(&mut buf, &host);

        // Larger than the primary region: forces an overflow block.
        let big: *mut u8 = arena.alloc(32, 8);
        assert!(!big.is_null());
        assert!(!arena.first_overflow.is_null());
        assert_eq!(ALLOC_COUNT.with(Cell::get), 1);

        // A second oversize allocation chains another block.
        let big2: *mut u8 = arena.alloc(64, 8);
        assert!(!big2.is_null());
        assert_eq!(ALLOC_COUNT.with(Cell::get), 2);

        arena.reset();
    }

    #[test]
    fn huge_alloc_allocates_exact_block() {
        reset_counters();
        let host: HostApi = test_host();
        let mut buf: [u8; 16] = [0; 16];
        let mut arena: CallArena = CallArena::new(&mut buf, &host);

        let huge: *mut u8 = arena.alloc(1 << 16, 8);
        assert!(!huge.is_null());
        assert_eq!(ALLOC_COUNT.with(Cell::get), 1);
        // SAFETY: the block header lives one ArenaOverflowBlock before nothing we
        // touch here; we only read capacity from the recorded chain head.
        let capacity: usize = unsafe { (*arena.first_overflow).capacity };
        assert!(capacity >= (1 << 16) + size_of::<ArenaOverflowBlock>());

        arena.reset();
    }

    #[test]
    fn reset_frees_exactly_what_was_allocated() {
        reset_counters();
        let host: HostApi = test_host();
        let mut buf: [u8; 16] = [0; 16];
        let mut arena: CallArena = CallArena::new(&mut buf, &host);

        let _a: *mut u8 = arena.alloc(32, 8);
        let _b: *mut u8 = arena.alloc(48, 8);
        let _c: *mut u8 = arena.alloc(64, 8);
        assert_eq!(ALLOC_COUNT.with(Cell::get), 3);

        arena.reset();
        assert_eq!(FREE_COUNT.with(Cell::get), 3);
        assert!(arena.first_overflow.is_null());

        // After reset the primary region is reusable.
        let again: *mut u8 = arena.alloc(8, 1);
        assert_eq!(again, arena.base);
    }
}
