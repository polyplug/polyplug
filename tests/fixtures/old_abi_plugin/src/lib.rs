//! Minimal ABI-v1 fixture used to verify that v2 loaders reject stale bundles.

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    1
}
