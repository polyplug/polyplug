//! Manifest — manifest.toml parsing for plugin bundles.
//!
//! Reads the companion `manifest.toml` for a plugin bundle before loading.
//! The `runtime` field determines which `BundleLoader` handles the bundle.
//! If absent, defaults to `"native"`.

use std::collections::HashMap;
use std::path::PathBuf;

/// Serialize this manifest to a TOML string.
///
/// This is the canonical way to create manifest.toml content.
/// Use this instead of manually formatting strings.
pub fn to_toml(manifest: &ManifestData) -> String {
    let mut out: String = String::new();

    out.push_str(&format!("id = {}\n", self.id));

    if !self.name.is_empty() {
        out.push_str(&format!("name = \"{}\"\n", self.name));
    }

    if !self.version.is_empty() {
        out.push_str(&format!("version = \"{}\"\n", self.version));
    }

    out.push_str(&format!("runtime = \"{}\"\n", self.runtime));

    if !self.file.is_empty() {
        out.push_str(&format!("file = \"{}\"\n", self.file));
    }

    if !self.provides.is_empty() {
        let provides: String = self
            .provides
            .iter()
            .map(|s: &String| format!("\"{}\"", s))
            .collect::<Vec<String>>()
            .join(", ");
        out.push_str(&format!("provides = [{}]\n", provides));
    }

    if self.needs_reinit_on_dep_reload {
        out.push_str("needs_reinit_on_dep_reload = true\n");
    }

    if !self.function_count.is_empty() {
        out.push_str("\n[function_count]\n");
        for (contract, count) in &self.function_count {
            out.push_str(&format!("\"{}\" = {}\n", contract, count));
        }
    }

    if !self.dependencies.is_empty() {
        for dep in &self.dependencies {
            out.push_str("\n[[dependency]]\n");
            out.push_str(&format!("kind = \"{}\"\n", dep.kind));
            out.push_str(&format!("contract = \"{}\"\n", dep.contract));
            out.push_str(&format!("min_version = \"{}\"\n", dep.min_version));
            if let Some(bundle) = &dep.bundle {
                out.push_str(&format!("bundle = \"{}\"\n", bundle));
            }
            if dep.contract_id != 0 {
                out.push_str(&format!("contract_id = {}\n", dep.contract_id));
            }
            if let Some(bundle_id) = dep.bundle_id {
                out.push_str(&format!("bundle_id = {}\n", bundle_id));
            }
        }
    }

    out
}

/// Parse a manifest from a TOML string.
///
/// This is the canonical way to parse manifest content.
/// Use this instead of calling `toml::from_str()` directly.
///
/// # Errors
/// Returns a `LoaderError::ManifestParse` if the TOML is malformed.
pub fn parse_from_str(toml_src: &str) -> Result<Self, crate::error::LoaderError> {
    let data: ManifestData = toml::from_str(toml_src).map_err(|e: toml::de::Error| {
        crate::error::LoaderError::ManifestParse {
            path: String::new(),
            reason: e.to_string(),
        }
    })?;
    Ok(data)
}
