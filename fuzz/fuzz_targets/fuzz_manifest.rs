#![no_main]

use libfuzzer_sys::fuzz_target;
use polyplug::loader::parse_manifest;
use polyplug_common::ManifestData;

// Fuzzes the runtime manifest parser (`polyplug::loader::parse_manifest`) and,
// on a successful parse, the `.validate()` path. `parse_manifest` reads a
// `manifest.toml` from a bundle directory, so the fuzzed bytes are written into
// a throwaway temp directory's `manifest.toml` before parsing. The target must
// never panic: a clean `Err` is the correct outcome for garbage input.
fuzz_target!(|data: &[u8]| {
    let content: &str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let dir: tempfile::TempDir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(_) => return,
    };

    let manifest_path: std::path::PathBuf = dir.path().join("manifest.toml");
    if std::fs::write(&manifest_path, content).is_err() {
        return;
    }

    if let Ok(manifest) = parse_manifest(dir.path()) {
        let _: Result<(), _> = ManifestData::validate(&manifest);
    }
});
