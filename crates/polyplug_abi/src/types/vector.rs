// TODO: FFI-safe Vec<T> type for returning owned collections across the FFI boundary.
// Will use host allocator (polyplug_host_alloc/polyplug_host_free) for memory management.
// See Array<T> for the current approach (caller-frees via host->free).
