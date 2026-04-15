//! Plugin module — types for plugin/guest contract metadata.
//!
//! Guest contracts are implemented by plugins and consumed by the host.

mod guest_contract_handle;
mod plugin_context;
mod plugin_descriptor;

pub use guest_contract_handle::GuestContractHandle;
pub use plugin_context::BundleInitContext;
pub use plugin_descriptor::PluginDescriptor;
