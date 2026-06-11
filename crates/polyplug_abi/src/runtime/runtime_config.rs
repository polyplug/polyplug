//! Runtime configuration.

use crate::runtime::Compatibility;
use crate::runtime::ReloadPhase;
use crate::runtime::UnloadMode;
use crate::types::LogLevel;
use crate::types::StringView;

/// Configuration for the polyplug runtime passed to `polyplug_runtime_create`.
///
/// # OWNERSHIP
/// Borrowed for the duration of the runtime build only.
/// The runtime copies any data it needs to retain.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Compatibility mode for version resolution.
    pub compatibility: Compatibility,
    /// How a bundle's loader-owned resources are reclaimed on unload.
    pub unload_mode: UnloadMode,
    /// Whether hot-reload is enabled.
    pub hot_reload_enabled: bool,
    /// Optional hot-reload callback, or null for no callback.
    ///
    /// The first argument is the opaque `on_reload_user_data` pointer, forwarded
    /// unchanged on every invocation. The second argument is a pointer to the
    /// [`ReloadPhase`] describing the phase.
    ///
    /// # Callback contract
    /// - The runtime ALWAYS passes a non-null, properly aligned `ReloadPhase`
    ///   pointer — callbacks never receive null.
    /// - The pointee (and the `StringView`s inside it) is valid only for the
    ///   duration of the call — copy the data to retain it.
    pub on_reload: Option<unsafe extern "C" fn(*mut core::ffi::c_void, *const ReloadPhase)>,
    /// Opaque user-data pointer forwarded to `on_reload` as its first argument.
    ///
    /// # Ownership
    /// Owned by the host that supplies the callback. The runtime never reads,
    /// writes, or frees the pointee — it only forwards the pointer.
    pub on_reload_user_data: *mut core::ffi::c_void,
    /// Optional logger callback, or null for the default behaviour.
    ///
    /// The runtime routes every diagnostic message through this callback as
    /// `(log_user_data, level, scope, message)`, where `level` is a
    /// [`LogLevel`] discriminant and `scope` is a short stable subsystem tag
    /// (examples: `"registry"`, `"loader.lua"`, `"reload"`).
    ///
    /// # Default (null callback)
    /// Messages at [`LogLevel::Error`] and [`LogLevel::Warn`] are written to
    /// stderr; all other levels are dropped. Hosts wanting full silence must
    /// install a no-op callback.
    ///
    /// # Callback contract
    /// - May be invoked from any thread.
    /// - Must NOT re-enter the runtime (calling any HostApi / runtime function
    ///   from inside the callback may deadlock).
    /// - The `scope` and `message` `StringView`s are valid only for the
    ///   duration of the call — copy the bytes to retain them.
    ///
    /// # Language note (LuaJIT hosts)
    /// The by-value `StringView` parameters are deliberate — the hot path
    /// stays copy-free. LuaJIT FFI callbacks cannot receive structs by value,
    /// so a Lua host cannot implement this signature directly; the Lua host
    /// SDK instead installs `polyplug_lua_log_trampoline` (exported by the
    /// polyplug_lua loader cdylib) here and carries a scalar-callback bridge
    /// in `log_user_data`.
    pub log: Option<unsafe extern "C" fn(*mut core::ffi::c_void, u32, StringView, StringView)>,
    /// Opaque user-data pointer forwarded to `log` as its first argument.
    ///
    /// # Ownership
    /// Owned by the host that supplies the callback. The runtime never reads,
    /// writes, or frees the pointee — it only forwards the pointer. The host
    /// must keep the pointee valid (and safe to use from any thread) for the
    /// runtime's entire lifetime.
    pub log_user_data: *mut core::ffi::c_void,
    /// Maximum [`LogLevel`] (as `u32`) delivered to the `log` callback.
    ///
    /// Messages with a level value greater than this are skipped before any
    /// formatting work is performed (zero cost for disabled levels). Ignored
    /// when `log` is null — the stderr default is always capped at
    /// [`LogLevel::Warn`].
    pub log_max_level: u32,
}

// SAFETY: RuntimeConfig contains function pointers, plain values, and opaque
// user-data pointers. Function pointers are Send. The user-data pointers are
// never dereferenced by the runtime — they are only forwarded to the host
// callbacks, and the host contract (documented on the fields) guarantees the
// pointees are valid and safe to use from any thread.
unsafe impl Send for RuntimeConfig {}
// SAFETY: No interior mutability — the struct is read-only after construction,
// and the opaque user-data pointers are only forwarded (never dereferenced)
// under the host's any-thread validity contract.
unsafe impl Sync for RuntimeConfig {}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            compatibility: Compatibility::Strict,
            unload_mode: UnloadMode::Retire,
            hot_reload_enabled: false,
            on_reload: None,
            on_reload_user_data: core::ptr::null_mut(),
            log: None,
            log_user_data: core::ptr::null_mut(),
            log_max_level: LogLevel::Warn as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use super::RuntimeConfig;
    use crate::runtime::Compatibility;
    use crate::runtime::UnloadMode;
    use crate::types::LogLevel;

    #[test]
    fn layout_runtime_config() {
        // compatibility: 4 bytes (u32) at 0x00
        // unload_mode: 4 bytes (u32) at 0x04
        // hot_reload_enabled: 1 byte (bool) at 0x08
        // padding: 7 bytes (0x09-0x0F)
        // on_reload: 8 bytes (fn pointer) at 0x10
        // on_reload_user_data: 8 bytes (pointer) at 0x18
        // log: 8 bytes (fn pointer) at 0x20
        // log_user_data: 8 bytes (pointer) at 0x28
        // log_max_level: 4 bytes (u32) at 0x30
        // padding: 4 bytes (0x34-0x37)
        // Total: 56 bytes, alignment 8
        assert_eq!(size_of::<RuntimeConfig>(), 56);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 0x0);
        assert_eq!(offset_of!(RuntimeConfig, unload_mode), 0x4);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_enabled), 0x8);
        assert_eq!(offset_of!(RuntimeConfig, on_reload), 0x10);
        assert_eq!(offset_of!(RuntimeConfig, on_reload_user_data), 0x18);
        assert_eq!(offset_of!(RuntimeConfig, log), 0x20);
        assert_eq!(offset_of!(RuntimeConfig, log_user_data), 0x28);
        assert_eq!(offset_of!(RuntimeConfig, log_max_level), 0x30);
    }

    /// The nullable callbacks are `Option<fn>`: the null-pointer optimization
    /// guarantees `Option<fn>` is layout-identical to a bare `fn` pointer (the
    /// niche IS the null fn pointer), so wrapping changes nothing at the ABI.
    /// This is the FFI-safety basis for the Option-nullability rule — it holds
    /// for fn pointers and references ONLY (not raw pointers, not structs).
    #[test]
    fn option_fn_pointer_niche_keeps_layout() {
        assert_eq!(
            size_of::<
                Option<
                    unsafe extern "C" fn(
                        *mut core::ffi::c_void,
                        *const crate::runtime::ReloadPhase,
                    ),
                >,
            >(),
            size_of::<
                unsafe extern "C" fn(*mut core::ffi::c_void, *const crate::runtime::ReloadPhase),
            >(),
        );
        assert_eq!(
            size_of::<
                Option<
                    unsafe extern "C" fn(
                        *mut core::ffi::c_void,
                        u32,
                        crate::types::StringView,
                        crate::types::StringView,
                    ),
                >,
            >(),
            size_of::<*const core::ffi::c_void>(),
        );
    }

    #[test]
    fn default_runtime_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.compatibility, Compatibility::Strict);
        assert_eq!(config.unload_mode, UnloadMode::Retire);
        assert!(!config.hot_reload_enabled);
        assert!(config.on_reload.is_none());
        assert!(config.on_reload_user_data.is_null());
        assert!(config.log.is_none());
        assert!(config.log_user_data.is_null());
        assert_eq!(config.log_max_level, LogLevel::Warn as u32);
    }
}
