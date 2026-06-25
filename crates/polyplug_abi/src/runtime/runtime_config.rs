//! Runtime configuration.

use core::ffi::c_void;
use core::ptr;

use crate::runtime::Compatibility;
use crate::runtime::ReloadPhase;
use crate::runtime::SignaturePolicy;
use crate::types::Array;
use crate::types::Ed25519PublicKey;
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
    pub on_reload: Option<unsafe extern "C" fn(*mut c_void, *const ReloadPhase)>,
    /// Opaque user-data pointer forwarded to `on_reload` as its first argument.
    ///
    /// # Ownership
    /// Owned by the host that supplies the callback. The runtime never reads,
    /// writes, or frees the pointee — it only forwards the pointer.
    pub on_reload_user_data: *mut c_void,
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
    pub log: Option<unsafe extern "C" fn(*mut c_void, u32, StringView, StringView)>,
    /// Opaque user-data pointer forwarded to `log` as its first argument.
    ///
    /// # Ownership
    /// Owned by the host that supplies the callback. The runtime never reads,
    /// writes, or frees the pointee — it only forwards the pointer. The host
    /// must keep the pointee valid (and safe to use from any thread) for the
    /// runtime's entire lifetime.
    pub log_user_data: *mut c_void,
    /// Maximum [`LogLevel`] (as `u32`) delivered to the `log` callback.
    ///
    /// Messages with a level value greater than this are skipped before any
    /// formatting work is performed (zero cost for disabled levels). Ignored
    /// when `log` is null — the stderr default is always capped at
    /// [`LogLevel::Warn`].
    pub log_max_level: u32,
    /// Bundle signature enforcement policy.
    ///
    /// Controls whether `bundle.sig` is required and how violations are handled.
    /// Defaults to [`SignaturePolicy::Off`], preserving existing behavior for
    /// unsigned bundles.
    ///
    /// # Layout note
    /// Placed at offset 0x2C (44), filling the 4-byte tail padding that existed
    /// after `log_max_level` at 0x28.
    pub signature_policy: SignaturePolicy,
    /// Host-configured trusted Ed25519 verifying-key allowlist (key pinning).
    ///
    /// Controls signing-key authenticity on top of the integrity guarantee that
    /// [`signature_policy`](Self::signature_policy) provides:
    ///
    /// - **Empty (default)** — Trust-On-First-Use (TOFU). The runtime trusts the
    ///   verifying key embedded in each bundle's `bundle.sig`; signature
    ///   verification proves the bundle is internally consistent and untampered,
    ///   but NOT that any particular author signed it.
    /// - **Non-empty** — Key pinning. After the normal Ed25519 verification, the
    ///   runtime additionally requires the bundle's embedded verifying key to be
    ///   a member of this allowlist; a bundle re-signed with an attacker key is
    ///   rejected even though its self-consistent signature is valid.
    ///
    /// Only public (verifying) keys are pinned — the private signing key stays
    /// offline. This field is read alongside `signature_policy`; with policy
    /// [`SignaturePolicy::Off`](crate::runtime::SignaturePolicy::Off) no
    /// verification runs and the allowlist is not consulted.
    ///
    /// # Ownership
    /// Borrowed for the duration of `polyplug_runtime_create` only. The runtime
    /// COPIES the key bytes it needs out of this buffer during construction and
    /// never retains the pointer — the host may free the backing storage as soon
    /// as `create` returns.
    ///
    /// # Layout note
    /// Placed at offset 0x30 (48), immediately after `signature_policy` plus its
    /// 4 bytes of pre-existing tail padding. The struct grows to 72 bytes,
    /// align 8 (the 24-byte `Array` raises the size from 48 to 72).
    pub trusted_keys: Array<Ed25519PublicKey>,
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
            hot_reload_enabled: false,
            on_reload: None,
            on_reload_user_data: ptr::null_mut(),
            log: None,
            log_user_data: ptr::null_mut(),
            log_max_level: LogLevel::Warn as u32,
            signature_policy: SignaturePolicy::Off,
            trusted_keys: Array::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use core::mem::{align_of, offset_of, size_of};

    use super::RuntimeConfig;
    use crate::runtime::{Compatibility, ReloadPhase, SignaturePolicy};
    use crate::types::{LogLevel, StringView};

    #[test]
    fn layout_runtime_config() {
        // compatibility: 4 bytes (u32) at 0x00
        // hot_reload_enabled: 1 byte (bool) at 0x04
        // padding: 3 bytes (0x05-0x07)
        // on_reload: 8 bytes (fn pointer) at 0x08
        // on_reload_user_data: 8 bytes (pointer) at 0x10
        // log: 8 bytes (fn pointer) at 0x18
        // log_user_data: 8 bytes (pointer) at 0x20
        // log_max_level: 4 bytes (u32) at 0x28
        // signature_policy: 4 bytes (u32) at 0x2C  ← fills former tail padding
        // trusted_keys: 24 bytes (Array<Ed25519PublicKey>) at 0x30
        // Total: 72 bytes, alignment 8
        assert_eq!(size_of::<RuntimeConfig>(), 72);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        // All pre-existing field offsets are unchanged by the trusted_keys
        // addition — the new field lands in the former tail, growing the struct
        // without disturbing any prior field.
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 0x0);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_enabled), 0x4);
        assert_eq!(offset_of!(RuntimeConfig, on_reload), 0x8);
        assert_eq!(offset_of!(RuntimeConfig, on_reload_user_data), 0x10);
        assert_eq!(offset_of!(RuntimeConfig, log), 0x18);
        assert_eq!(offset_of!(RuntimeConfig, log_user_data), 0x20);
        assert_eq!(offset_of!(RuntimeConfig, log_max_level), 0x28);
        assert_eq!(offset_of!(RuntimeConfig, signature_policy), 0x2C);
        assert_eq!(offset_of!(RuntimeConfig, trusted_keys), 0x30);
    }

    /// The nullable callbacks are `Option<fn>`: the null-pointer optimization
    /// guarantees `Option<fn>` is layout-identical to a bare `fn` pointer (the
    /// niche IS the null fn pointer), so wrapping changes nothing at the ABI.
    /// This is the FFI-safety basis for the Option-nullability rule — it holds
    /// for fn pointers and references ONLY (not raw pointers, not structs).
    #[test]
    fn option_fn_pointer_niche_keeps_layout() {
        assert_eq!(
            size_of::<Option<unsafe extern "C" fn(*mut c_void, *const ReloadPhase)>>(),
            size_of::<unsafe extern "C" fn(*mut c_void, *const ReloadPhase)>(),
        );
        assert_eq!(
            size_of::<Option<unsafe extern "C" fn(*mut c_void, u32, StringView, StringView)>>(),
            size_of::<*const c_void>(),
        );
    }

    #[test]
    fn default_runtime_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.compatibility, Compatibility::Strict);
        assert!(!config.hot_reload_enabled);
        assert!(config.on_reload.is_none());
        assert!(config.on_reload_user_data.is_null());
        assert!(config.log.is_none());
        assert!(config.log_user_data.is_null());
        assert_eq!(config.log_max_level, LogLevel::Warn as u32);
        assert_eq!(config.signature_policy, SignaturePolicy::Off);
        // Default = TOFU: an empty trusted-key allowlist so existing zero-init
        // hosts are unaffected and no pinning is enforced.
        assert!(config.trusted_keys.is_empty());
    }
}
