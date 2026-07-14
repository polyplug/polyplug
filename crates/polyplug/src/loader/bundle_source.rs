use std::path::PathBuf;

/// Where a bundle's executable artifact comes from.
///
/// Loaders receive this value while acquiring an executable artifact. Loaded bundle
/// descriptors retain only its payload-free [`BundleOrigin`] for host introspection.
/// `Internal` has no executable artifact because generated bindings register its
/// providers directly.
///
/// [`BundleLoader::load`]: crate::loader::BundleLoader::load
///
/// # Loader support
///
/// - The **native** loader supports [`BundleSource::Path`] only. There is no clean,
///   portable in-memory `dlopen` on Windows/macOS, so `Code`/`Bytes` are rejected
///   by the native loader.
/// - VM loaders (Lua, JS, Python) gain real [`BundleSource::Code`] support, and the
///   .NET loader gains real [`BundleSource::Bytes`] support, in a later phase. Until
///   then every loader rejects the variants it does not yet implement with a
///   structured error.
///
/// # No bundle directory for non-path sources
///
/// [`BundleSource::Code`] and [`BundleSource::Bytes`] carry no bundle directory, so
/// directory-relative provisioning — such as prepending the bundle directory to the
/// Lua `package.path` or the Python `sys.path` — does not apply. These sources are
/// **single-file only**: the artifact must be self-contained, with no sibling files
/// resolved relative to a bundle directory.
#[derive(Debug, Clone)]
pub enum BundleSource {
    /// Providers registered directly by generated bindings in the host process.
    Internal,
    /// A bundle directory on disk. The loader resolves the plugin file relative to
    /// this directory using the manifest's `file` field (path-based loading).
    Path(PathBuf),
    /// In-memory VM source text (e.g. Lua, JavaScript, or Python). Single-file only:
    /// there is no bundle directory for relative `require`/`import` resolution.
    Code(String),
    /// Raw artifact bytes (e.g. a .NET assembly). Single-file only: there is no
    /// bundle directory for relative file resolution.
    Bytes(Vec<u8>),
}

/// Payload-free origin metadata retained by loaded bundle descriptors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleOrigin {
    Internal,
    Path(PathBuf),
    Code,
    Bytes,
}

impl BundleSource {
    /// Return payload-free origin metadata suitable for public introspection.
    pub fn origin(&self) -> BundleOrigin {
        match self {
            BundleSource::Internal => BundleOrigin::Internal,
            BundleSource::Path(path) => BundleOrigin::Path(path.clone()),
            BundleSource::Code(_) => BundleOrigin::Code,
            BundleSource::Bytes(_) => BundleOrigin::Bytes,
        }
    }

    /// A short, stable label identifying the variant for diagnostics and
    /// introspection (`"internal"`, `"path"`, `"code"`, or `"bytes"`).
    pub fn kind(&self) -> &'static str {
        match self {
            BundleSource::Internal => "internal",
            BundleSource::Path(_) => "path",
            BundleSource::Code(_) => "code",
            BundleSource::Bytes(_) => "bytes",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BundleOrigin, BundleSource};
    use std::path::PathBuf;

    #[test]
    fn origin_retains_only_path_metadata() {
        let path: PathBuf = "/bundles/example".into();
        assert_eq!(
            BundleSource::Path(path.clone()).origin(),
            BundleOrigin::Path(path)
        );
        assert_eq!(BundleSource::Internal.origin(), BundleOrigin::Internal);

        let code = BundleSource::Code("private source text".to_owned());
        let code_origin = code.origin();
        assert_eq!(code_origin, BundleOrigin::Code);
        assert!(!format!("{code_origin:?}").contains("private source text"));
        assert!(matches!(code, BundleSource::Code(source) if source == "private source text"));

        let bytes = BundleSource::Bytes(vec![0x50, 0x50, 0x4C, 0x47]);
        let bytes_origin = bytes.origin();
        assert_eq!(bytes_origin, BundleOrigin::Bytes);
        assert!(!format!("{bytes_origin:?}").contains("80, 80, 76, 71"));
        assert!(
            matches!(bytes, BundleSource::Bytes(payload) if payload == [0x50, 0x50, 0x4C, 0x47])
        );
    }
}
