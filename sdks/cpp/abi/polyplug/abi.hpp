#pragma once
#include <cstdint>
#include <cstddef>

///  Non-owning UTF-8 string view.
///
///  OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
///  of the call. Never freed by the receiver.
struct StringView {
    ///  UTF-8 bytes, NOT null-terminated.
    const uint8_t* ptr;
    ///  Byte count.
    size_t len;
};

///  Owning byte buffer.
///
///  OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
///  Owner calls `polyplug_host_free(ptr, cap, align)` when done.
struct Buffer {
    uint8_t* ptr;
    ///  Bytes currently used.
    size_t len;
    ///  Bytes allocated.
    size_t cap;
};

///  ABI error — returned by value from all ABI calls.
///
///  OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
///  via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
///  after reading. If `code == ABI_OK`, `message.ptr` is NULL — no free needed.
struct AbiError {
    ///  0 = success, non-zero = error.
    uint32_t code;
    ///  Empty/NULL if success. UTF-8 message if non-zero code.
    StringView message;
};

///  Opaque handle to a loaded plugin — validated on use.
///
///  INTERNAL STRUCTURE: index into registry array + generation counter.
///  The generation counter detects use-after-unload.
struct PluginHandle {
    ///  Slot in the registry array.
    uint32_t index;
    ///  Incremented on unload — detects stale handles.
    uint32_t generation;
};

///  Opaque host context passed to plugin functions via rt_ctx parameter.
///
///  Contains the runtime pointer and the bundle_id of the calling bundle.
///  The actual implementation is in the polyplug crate; this definition
///  establishes the ABI layout.
///
///  OWNERSHIP: `'static`, lives as long as the runtime.
struct HostContext {
    ///  Opaque pointer to the Runtime. Never dereferenced by plugins.
    void* runtime;
    ///  Bundle ID of the calling bundle for dependency enforcement.
    uint64_t bundle_id;
};

///  Native dispatch data — direct function pointer array.
///
///  Used when `dispatch_type == DispatchType::Native`.
///  The `functions` array contains `function_count` function pointers.
struct NativeDispatch {
    ///  Pointer to a static array of function pointers, indexed by function_id.
    void* const* functions;
};

///  VM dispatch data — call through a dispatch function.
///
///  Used when `dispatch_type == DispatchType::VirtualMachine`.
///  The `call` function receives `loader_data` which contains VM-specific state.
struct VmDispatch {
    ///  Dispatch function called for every VM function invocation.
    ///
    ///  # Arguments
    ///  - `loader_data`: VM-specific data (cast from `*mut c_void`)
    ///  - `fn_id`: Function index within the contract
    ///  - `args`: Pointer to packed arguments (ABI-specific layout)
    ///  - `out`: Pointer to output buffer for return value
    AbiError (*call )(void*, uint32_t, const void*, void*);
    ///  Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
    ///  Opaque to the host; interpreted by the dispatch function.
    void* loader_data;
};

///  Plugin interface — one per contract implemented by a plugin.
///
///  OWNERSHIP: Must be `'static` or intentionally leaked.
///  Never stack-allocated. Never freed while runtime lives.
///
///  # Dispatch
///  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
///  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
struct PluginInterface {
    ///  Pointer to the host context for this plugin.
    ///  Used for host function calls and dependency enforcement.
    const HostContext* rt_ctx;
    ///  FNV-1a hash of "contract_name@major_version".
    uint64_t contract_id;
    ///  minor.patch encoded as `(minor << 16 | patch)`.
    uint32_t contract_version;
    ///  Number of valid entries in the dispatch array.
    uint32_t function_count;
    ///  Dispatch mechanism type (Native or VirtualMachine).
    DispatchType dispatch_type;
    ///  Union of dispatch mechanisms — access based on dispatch_type.
    PluginDispatch dispatch;
};

///  Host contract vtable header — metadata for a host-provided contract.
struct HostContractVTableHeader {
    ///  VTable format version (for future compatibility).
    uint32_t vtable_version;
    ///  FNV-1a hash of "contract_name@major_version".
    uint64_t contract_id;
    ///  Contract major version.
    uint32_t contract_major;
    ///  Contract minor version.
    uint32_t contract_minor;
    ///  Number of functions in this contract.
    uint32_t function_count;
    ///  Dispatch mechanism type (Native or VirtualMachine).
    DispatchType dispatch_type;
};

///  Native dispatch for host contracts — direct function pointer array.
///
///  Used when `dispatch_type == DispatchType::Native`.
///  The `functions` array contains `function_count` function pointers.
struct NativeHostContractDispatch {
    ///  Pointer to the implementation (e.g., Box<dyn Trait> as *const c_void).
    ///  This is passed as the first argument to all native dispatch functions.
    const void* impl_ptr;
    ///  Pointer to a static array of function pointers, indexed by function_id.
    void* const* functions;
};

///  VM dispatch for host contracts — call through a dispatch function.
///
///  Used when `dispatch_type == DispatchType::VirtualMachine`.
///  The `call` function receives `bridge_data` which contains VM-specific state.
struct VmHostContractDispatch {
    ///  Dispatch function called for every VM function invocation.
    ///
    ///  # Arguments
    ///  - `bridge_data`: VM-specific data (cast from `*mut c_void`)
    ///  - `fn_id`: Function index within the contract
    ///  - `args`: Pointer to packed arguments (ABI-specific layout)
    ///  - `out`: Pointer to output buffer for return value
    AbiError (*call )(void*, uint32_t, const void*, void*);
    ///  VM-specific data (opaque to the host; interpreted by the dispatch function).
    void* bridge_data;
};

///  Host contract vtable — complete interface for a host-provided contract.
///
///  OWNERSHIP: Must be `'static` or intentionally leaked.
///  Never stack-allocated. Never freed while runtime lives.
///
///  # Dispatch
///  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
///  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
struct HostContractVTable {
    ///  Header containing contract metadata.
    HostContractVTableHeader header;
    ///  Union of dispatch mechanisms — access based on dispatch_type.
    HostContractDispatch dispatch;
};

///  Host capabilities passed to every plugin at init time.
///
///  OWNERSHIP: `'static`, lives as long as the runtime.
///
///  All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
///  This allows each Runtime to have its own isolated state (no global registry).
struct HostVTable {
    AbiError (*register_plugin )(void*, const PluginDescriptor*, const PluginInterface*);
    uint8_t* (*alloc )(void*, size_t, size_t);
    void (*free )(void*, uint8_t*, size_t, size_t);
    PluginHandle (*find_by_contract )(void*, uint64_t, uint32_t);
    PluginHandle (*find_by_bundle )(void*, uint64_t, uint64_t, uint32_t);
    size_t (*find_all_by_contract )(void*, uint64_t, uint32_t, PluginHandle*, size_t);
    const PluginInterface* (*resolve_plugin )(void*, PluginHandle);
    ///  Get host contract vtable by contract_id and minimum version.
    ///  Returns null if no host contract matches the criteria.
    const HostContractVTable* (*get_host_contract )(void*, uint64_t, uint32_t);
};

///  Metadata about a plugin within a bundle.
///
///  OWNERSHIP: value type passed by pointer during init. The `name` and
///  `contract_name` StringViews are borrowed from the plugin's static memory.
///  The receiver must not free or outlive the plugin's library.
struct PluginDescriptor {
    ///  Human-readable plugin name.
    StringView name;
    ///  Full contract name for collision detection.
    StringView contract_name;
    uint32_t version_major;
    uint32_t version_minor;
    uint32_t version_patch;
};

///  Context passed to every guest `polyplug_init()` function.
///  The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
///  **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
struct PluginContext {
    ///  Absolute canonical path to the directory containing the loaded bundle.
    StringView bundle_path;
    ///  Host's supported ABI version for negotiation (Option C).
    ///  Plugin can use this to determine available features.
    uint32_t host_abi_version;
    ///  Bundle ID for dependency enforcement during init.
    uint64_t bundle_id;
};

///  Configuration passed to `polyplug_runtime_create` during runtime initialisation.
///
///  OWNERSHIP: borrowed for the duration of the runtime build only.
///  The caller may free all pointed-to memory after the build
///  returns. The runtime copies any data it needs to retain.
struct RuntimeConfig {
    ///  Plugin directories to scan (array of `plugin_dir_count` StringViews).
    const StringView* plugin_dirs;
    size_t plugin_dir_count;
    ///  Compatibility mode: 0 = Strict (only mode implemented in MVP).
    uint32_t compatibility;
};

///  Dispatch mechanism type — determines how function calls are routed.
enum class DispatchType : uint32_t {
    ///  Native dispatch: direct function pointer calls (zero overhead).
    Native = 0,
    ///  VM dispatch: call through a dispatch function with loader_data.
    VirtualMachine = 1,
};

///  Host runtime type identifier — identifies the language/runtime hosting plugins.
enum class HostRuntime : uint8_t {
    Rust = 0,
    Python = 1,
    Lua = 2,
    JavaScript = 3,
};

///  Union of dispatch mechanisms — use based on `dispatch_type`.
///
///  # Safety
///  Access the correct variant based on `PluginInterface::dispatch_type`:
///  - `dispatch_type == Native` → access `.native`
///  - `dispatch_type == VirtualMachine` → access `.vm`
union PluginDispatch {
    NativeDispatch native;
    VmDispatch vm;
};

///  Union of host contract dispatch mechanisms — use based on `dispatch_type`.
///
///  # Safety
///  Access the correct variant based on `HostContractVTableHeader::dispatch_type`:
///  - `dispatch_type == Native` → access `.native`
///  - `dispatch_type == VirtualMachine` → access `.vm`
union HostContractDispatch {
    NativeHostContractDispatch native;
    VmHostContractDispatch vm;
};

StringView string_view_from_static(&'static[u8] bytes);

StringView string_view_null();

&str string_view_as_str(&StringView sv);

String string_view_to_string_owned(&StringView sv);

&[u8] buffer_as_slice(&Buffer buf);

&mut[u8] buffer_as_mut_slice(&mutBuffer buf);

AbiError abi_error_ok();

AbiError abi_error_panic_caught();

bool abi_error_is_ok(&AbiError err);

PluginHandle plugin_handle_null();

bool plugin_handle_is_null(&PluginHandle handle);

#define POLYPLUG_ABI_VERSION 1U
#define ABI_OK 0U
#define ABI_ERROR_GENERIC 1U
#define ABI_BUFFER_TOO_SMALL 2U
#define ABI_ERROR_PANIC 3U
#define ABI_ERROR_NOT_FOUND 4U
#define ABI_ERROR_STALE_HANDLE 5U
#define ABI_FUNCTION_NOT_AVAIL 6U
#define ABI_ERROR_DUPLICATE_PROVIDER 7U
#define ABI_ERROR_INVALID_POINTER 8U
#define ABI_HOST_CONTRACT_NOT_FOUND 100U
#define ABI_HOST_CONTRACT_VERSION_MISMATCH 101U
#define ABI_HOST_CONTRACT_CALL_FAILED 102U
constexpr uint64_t fnv1a_64(&[u8] data) { /* implementation */ }

constexpr uint64_t contract_id(&str name, uint32_t major) { /* implementation */ }

constexpr uint64_t bundle_id(&str name) { /* implementation */ }

constexpr uint64_t host_contract_id(&str name, uint32_t major) { /* implementation */ }

constexpr uint64_t plugin_contract_id(&str name, uint32_t major) { /* implementation */ }

