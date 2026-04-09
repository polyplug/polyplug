//! Plugin module — types for plugin/guest contract metadata.
//!
//! Guest contracts are implemented by plugins and consumed by the host.

mod plugin_context;
mod plugin_descriptor;
mod guest_contract_handle;
mod guest_contract_interface;

pub use plugin_context::PluginContext;
pub use plugin_descriptor::PluginDescriptor;
pub use guest_contract_handle::GuestContractHandle;
pub use guest_contract_interface::GuestContractInterface;