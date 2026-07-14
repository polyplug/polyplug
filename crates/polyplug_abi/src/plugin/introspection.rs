use polyplug_utils::BundleId;

use crate::SupportedLanguage;
use crate::host::HostApi;
use crate::plugin::GuestContractHandle;
use crate::types::{Array, Version};

/// The retained origin of a loaded bundle.
///
/// This is metadata only: a `Code` or `Bytes` origin never exposes the artifact
/// payload through the ABI.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleSourceKind {
    /// Providers registered directly by generated bindings in the host process.
    Internal = 0,
    /// An on-disk bundle directory.
    Path = 1,
    /// In-memory source text.
    Code = 2,
    /// In-memory artifact bytes.
    Bytes = 3,
}

/// Caller-owned ABI descriptor of one loaded bundle.
///
/// `name` is allocated through `HostApi::alloc`. The successful callback transfers
/// ownership to the caller, which must release a non-null buffer through
/// `HostApi::free(host, name.items, name.len, name.align)` exactly once.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BundleDescriptorView {
    /// Stable bundle identity.
    pub id: BundleId,
    /// Human-readable bundle name.
    pub name: Array<u8>,
    /// Semantic bundle version.
    pub version: Version,
    /// Runtime language selected for the bundle.
    pub runtime: SupportedLanguage,
    /// Retained bundle origin.
    pub source_kind: BundleSourceKind,
}

/// Caller-owned ABI descriptor for a registered provider.
///
/// Each non-null string buffer is allocated through `HostApi::alloc` and must be
/// released through `HostApi::free` with its exact `len` and `align` exactly once.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OwnedPluginDescriptorView {
    /// Human-readable provider name.
    pub name: Array<u8>,
    /// Full guest-contract name.
    pub contract_name: Array<u8>,
    /// Provider version.
    pub version: Version,
}

/// Caller-owned ABI descriptor of one registered guest-contract provider.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RegisteredContractDescriptorView {
    /// Stable handle for the live registration.
    pub handle: GuestContractHandle,
    /// Bundle that owns the registration.
    pub bundle_id: BundleId,
    /// Canonical guest-contract identity.
    pub contract_id: u64,
    /// Canonical provider descriptor with caller-owned string buffers.
    pub plugin: OwnedPluginDescriptorView,
}

/// `HostApi::reserved` points to this table for current runtimes. Its address is
/// stable for the runtime lifetime; SDKs must treat a null `reserved` pointer as
/// unsupported. A successful descriptor callback transfers its string allocations
/// to the caller; a failed callback transfers no allocation.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RuntimeIntrospection {
    /// Copy a loaded bundle descriptor into `out_descriptor`.
    pub get_bundle_descriptor: unsafe extern "C" fn(
        host: *const HostApi,
        bundle_id: BundleId,
        out_descriptor: *mut BundleDescriptorView,
    ) -> bool,
    /// Write stable handles for all live guest-contract registrations into `out_handles`.
    ///
    /// A non-null `out_handles` receives an owned array; an empty result is
    /// `Array::empty()`. This explicit output avoids aggregate-return lowering.
    pub list_registered_guest_contracts:
        unsafe extern "C" fn(host: *const HostApi, out_handles: *mut Array<GuestContractHandle>),
    /// Copy a live guest-contract ownership descriptor into `out_descriptor`.
    pub get_registered_contract_descriptor: unsafe extern "C" fn(
        host: *const HostApi,
        handle: GuestContractHandle,
        out_descriptor: *mut RegisteredContractDescriptorView,
    ) -> bool,
}
