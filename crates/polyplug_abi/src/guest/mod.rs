//! Guest module — types for guest (plugin) contracts.
//!
//! Guest contracts are implemented by plugins and consumed by the host.

mod guest_contract_instance;
mod guest_contract_interface;

pub use guest_contract_instance::GuestContractInstance;
pub use guest_contract_interface::GuestContractInterface;