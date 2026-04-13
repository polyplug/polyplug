use std::path::PathBuf;

/// A successfully loaded bundle.
///
/// Note: Library handles are owned by the loader (e.g., NativeLoader),
/// not by this struct. The registry only stores interface pointers.
pub(crate) struct LoadedBundle {
    pub path: PathBuf,
}
