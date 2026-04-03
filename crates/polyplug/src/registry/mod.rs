pub mod plugin_registry;

pub(crate) use plugin_registry::PluginRegistry;
pub use plugin_registry::PluginGuard;
pub use plugin_registry::VTableSlot;