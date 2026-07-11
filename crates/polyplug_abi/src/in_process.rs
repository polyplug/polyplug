//! Canonical registration input for host-created in-process bundles.
//!
//! A host retains language-specific implementation objects in its own runtime-local
//! resident. This ABI carries only copied metadata, dependency IDs, and guest interface
//! tables; core never receives or interprets an implementation-object pointer.

use core::ffi::c_void;

use crate::types::StringView;
use crate::{
    AbiError, AbiErrorCode, GuestContractInterface, HostApi, PluginDescriptor, SupportedLanguage,
    Version,
};

/// Metadata shared by every contract in one in-process bundle registration.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InProcessBundleMetadata {
    /// Stable UTF-8 bundle name. Core derives the nonzero bundle ID from this value.
    pub name: StringView,
    /// Bundle semantic version.
    pub version: Version,
    /// Language owning the resident and interface implementation.
    pub runtime: SupportedLanguage,
}

/// One contract supplied by an in-process bundle.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InProcessContractRegistration {
    /// Provider and contract metadata copied by core during registration.
    pub descriptor: PluginDescriptor,
    /// Canonical guest interface table copied and validated by core during registration.
    pub interface: *const GuestContractInterface,
    /// Opaque generated-adapter context copied into the registered interface.
    ///
    /// Core never dereferences, writes, or frees this pointer. Generated lifecycle
    /// and dispatch thunks receive it as their first callback argument.
    pub adapter_context: *mut c_void,
}

// SAFETY: registration tables are immutable borrowed ABI data. Core copies their
// contents synchronously and never mutates through the interface pointer.
unsafe impl Send for InProcessContractRegistration {}

// SAFETY: see the Send implementation; sharing a table never grants mutation.
unsafe impl Sync for InProcessContractRegistration {}

/// Complete, one-shot in-process bundle registration input.
///
/// All pointers are borrowed only for the synchronous registration call. If a count is
/// nonzero, its corresponding pointer is required to be non-null and valid for that many
/// elements. `dependency_ids` contains canonical `GuestContractId` numeric values.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InProcessBundleRegistration {
    /// Bundle-level metadata.
    pub metadata: InProcessBundleMetadata,
    /// Declared guest-contract dependency IDs.
    pub dependency_ids: *const u64,
    /// Number of dependency IDs.
    pub dependency_count: usize,
    /// Contract descriptors and interface-table pointers.
    pub contracts: *const InProcessContractRegistration,
    /// Number of supplied contracts.
    pub contract_count: usize,
}

/// Deterministic rejection callback for standalone HostApi test tables.
///
/// Production runtimes install their own registration callback.
///
/// # Safety
///
/// Non-null output pointers must be valid and writable for their respective
/// values. The registration pointer is not dereferenced by this rejecting stub.
pub unsafe extern "C" fn reject_in_process_bundle(
    _this: *const HostApi,
    _registration: *const InProcessBundleRegistration,
    out_bundle_id: *mut u64,
    out_err: *mut AbiError,
) {
    if !out_bundle_id.is_null() {
        // SAFETY: out_bundle_id is non-null and supplied by the caller.
        unsafe { out_bundle_id.write(0) };
    }
    if !out_err.is_null() {
        // SAFETY: out_err is non-null and supplied by the caller.
        unsafe {
            out_err.write(AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            })
        };
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use super::{
        InProcessBundleMetadata, InProcessBundleRegistration, InProcessContractRegistration,
    };

    #[test]
    fn layout_in_process_registration() {
        assert_eq!(size_of::<InProcessBundleMetadata>(), 32);
        assert_eq!(align_of::<InProcessBundleMetadata>(), 8);
        assert_eq!(offset_of!(InProcessBundleMetadata, name), 0);
        assert_eq!(offset_of!(InProcessBundleMetadata, version), 16);
        assert_eq!(offset_of!(InProcessBundleMetadata, runtime), 28);

        assert_eq!(size_of::<InProcessContractRegistration>(), 64);
        assert_eq!(align_of::<InProcessContractRegistration>(), 8);
        assert_eq!(offset_of!(InProcessContractRegistration, descriptor), 0);
        assert_eq!(offset_of!(InProcessContractRegistration, interface), 48);
        assert_eq!(
            offset_of!(InProcessContractRegistration, adapter_context),
            56
        );

        assert_eq!(size_of::<InProcessBundleRegistration>(), 64);
        assert_eq!(align_of::<InProcessBundleRegistration>(), 8);
        assert_eq!(offset_of!(InProcessBundleRegistration, metadata), 0);
        assert_eq!(offset_of!(InProcessBundleRegistration, dependency_ids), 32);
        assert_eq!(
            offset_of!(InProcessBundleRegistration, dependency_count),
            40
        );
        assert_eq!(offset_of!(InProcessBundleRegistration, contracts), 48);
        assert_eq!(offset_of!(InProcessBundleRegistration, contract_count), 56);
    }
}
