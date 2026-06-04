//! Build script for `polyplug_python`.
//!
//! Emits the linked CPython library name (e.g. `python3.14`) as the
//! `POLYPLUG_PYTHON_LIB_NAME` compile-time environment variable so the loader
//! can promote libpython's symbols into the global namespace at runtime when
//! the loader cdylib is `dlopen`ed by a non-Python host (see `context.rs`).

fn main() {
    // pyo3-build-config resolves the same interpreter pyo3 links against.
    let lib_name: String = pyo3_build_config::get()
        .lib_name
        .clone()
        .unwrap_or_default();
    println!("cargo:rustc-env=POLYPLUG_PYTHON_LIB_NAME={lib_name}");
    println!("cargo:rerun-if-changed=build.rs");
}
