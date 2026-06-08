//! Unload mode — how a bundle's loader resources are reclaimed on unload.

/// How a bundle's loader-owned resources are handled when it is unloaded.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnloadMode {
    /// Retire-not-drop: the loader keeps the bundle's library/VM mapped for the
    /// runtime's lifetime after unload. Any raw function pointer already resolved
    /// from the bundle stays valid. This is the default, fully-safe behaviour.
    Retire = 0,
    /// Reclaim: the loader frees the bundle's library/VM at unload (e.g. native
    /// `dlclose`), releasing OS resources and the on-disk file lock so a developer
    /// can rebuild and reload the bundle. Host-coordinated: the host must guarantee
    /// no thread is calling, or holds a pointer into, the bundle when it is unloaded.
    Reclaim = 1,
}

#[cfg(test)]
mod tests {
    use super::UnloadMode;
    use core::mem::size_of;

    #[test]
    fn unload_mode_repr_u32() {
        assert_eq!(UnloadMode::Retire as u32, 0);
        assert_eq!(UnloadMode::Reclaim as u32, 1);
        assert_eq!(size_of::<UnloadMode>(), 4);
    }
}
