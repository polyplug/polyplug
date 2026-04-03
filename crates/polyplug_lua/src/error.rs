//! Lua-specific error types.

use thiserror::Error;

/// Errors from the Lua loader.
#[derive(Debug, Error)]
pub enum LuaLoaderError {
    #[error("lua vm init failed: {reason}")]
    LuaVmInitFailed { reason: String },

    #[error("lua script load failed: path={path}, reason={reason}")]
    LuaScriptLoadFailed { path: String, reason: String },

    #[error("lua plugin missing polyplug_init function: bundle={bundle}")]
    LuaInitFunctionMissing { bundle: String },

    #[error("lua polyplug_init raised error: bundle={bundle}, message={message}")]
    LuaInitRaisedError { bundle: String, message: String },
}