use std::path::PathBuf;

/// Where a bundle's executable artifact comes from.
///
/// `BundleSource` is **host-internal**: it never crosses the C ABI and is not part
/// of the frozen ABI surface. It is threaded through [`BundleLoader::load`] so a
/// loader can serve a bundle from an on-disk directory, from in-memory VM source
/// text, or from raw artifact bytes.
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

impl BundleSource {
    /// A short, stable label identifying the variant, for diagnostics and error
    /// messages (`"path"`, `"code"`, or `"bytes"`).
    pub fn kind(&self) -> &'static str {
        match self {
            BundleSource::Path(_) => "path",
            BundleSource::Code(_) => "code",
            BundleSource::Bytes(_) => "bytes",
        }
    }
}
