// Build script for showcase-host.
// Emits -export-dynamic so polyplug_host_alloc and polyplug_host_free
// are visible to plugin .so files loaded via dlopen at runtime.

fn main() {
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg=-Wl,-export-dynamic");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-rdynamic");
}
