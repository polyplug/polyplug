//! Integration tests: malformed bundle inputs must return clean Err, never panic.

use std::fs;
use std::path::PathBuf;

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_last_error;
use polyplug::ffi::polyplug_load_bundle;
use polyplug::ffi::polyplug_runtime_free;
use polyplug::ffi::polyplug_runtime_new;

fn load_bundle_path(rt: *mut OpaqueRuntime, dir: &str) -> u32 {
    let bytes: &[u8] = dir.as_bytes();
    // SAFETY: rt non-null (checked by caller), bytes valid for bytes.len().
    unsafe { polyplug_load_bundle(rt, bytes.as_ptr(), bytes.len()) }
}

fn make_tmpdir(name: &str) -> PathBuf {
    let base: PathBuf = std::env::temp_dir().join(format!("polyplug_test_{name}"));
    fs::create_dir_all(&base).expect("create tmpdir");
    base
}

fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn test_truncated_so() {
    // SAFETY: polyplug_runtime_new() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("truncated");
    // Write a truncated .so: valid ELF magic + 508 zero bytes (truncated body)
    let mut so: Vec<u8> = vec![0x7f_u8, b'E', b'L', b'F'];
    so.extend_from_slice(&[0u8; 508]);
    fs::write(dir.join("libtruncated.so"), &so).expect("write truncated so");
    fs::write(
        dir.join("manifest.toml"),
        b"bundle_name = \"truncated\"\nruntime = \"rust\"\nfile = \"libtruncated.so\"\n",
    )
    .expect("write manifest");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8 path"));
    assert_ne!(rc, 0, "truncated .so must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_new().
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_wrong_magic_bytes() {
    // SAFETY: polyplug_runtime_new() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("wrong_magic");
    let garbage: Vec<u8> = b"NOTANELF\x00".iter().cycle().take(512).cloned().collect();
    fs::write(dir.join("libwrong.so"), &garbage).expect("write garbage");
    fs::write(
        dir.join("manifest.toml"),
        b"bundle_name = \"wrong_magic\"\nruntime = \"rust\"\nfile = \"libwrong.so\"\n",
    )
    .expect("write manifest");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
    assert_ne!(rc, 0, "wrong magic bytes must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_new().
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_missing_init_symbol() {
    // Uses the no_init_plugin fixture built by build.rs.
    // NO_INIT_PLUGIN_DIR env var is set by build.rs.
    let dir: &str = env!("NO_INIT_PLUGIN_DIR");
    // SAFETY: polyplug_runtime_new() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let rc: u32 = load_bundle_path(rt, dir);
    assert_ne!(
        rc, 0,
        "plugin missing polyplug_init must produce non-zero return"
    );
    // Verify error message mentions the missing symbol
    let mut buf: [u8; 256] = [0u8; 256];
    // SAFETY: buf valid for 256 bytes, polyplug_last_error writes at most buf_len bytes.
    let n: usize = unsafe { polyplug_last_error(buf.as_mut_ptr(), buf.len()) };
    let msg: &str = core::str::from_utf8(&buf[..n]).expect("last_error is valid utf8");
    assert!(
        msg.contains("polyplug_init") || msg.contains("symbol") || msg.contains("init"),
        "error message should mention missing symbol, got: {}",
        msg
    );
    // SAFETY: rt was returned by polyplug_runtime_new().
    unsafe { polyplug_runtime_free(rt) };
}

// Test d (ABI mismatch) is intentionally omitted.
// A plugin that exports init() with a wrong signature causes undefined behaviour
// at the call site. This is documented in polyplug_prd.md section 27 as out-of-scope.
// There is no safe way to test this in-process.

#[test]
fn test_so_file_missing_from_bundle() {
    // SAFETY: polyplug_runtime_new() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("missing_so");
    // Write manifest pointing to a nonexistent .so
    fs::write(
        dir.join("manifest.toml"),
        b"bundle_name = \"missing_so\"\nruntime = \"rust\"\nfile = \"nonexistent.so\"\n",
    )
    .expect("write manifest");
    // Do NOT create nonexistent.so
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
    assert_ne!(rc, 0, "missing .so file must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_new().
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_unknown_runtime() {
    // SAFETY: polyplug_runtime_new() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("unknown_runtime");
    // Create a dummy .so file so the manifest parse succeeds
    fs::write(dir.join("dummy.so"), b"notareal").expect("write dummy");
    fs::write(
        dir.join("manifest.toml"),
        b"bundle_name = \"unknown_runtime\"\nruntime = \"cobol\"\nfile = \"dummy.so\"\n",
    )
    .expect("write manifest");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
    assert_ne!(rc, 0, "unknown runtime must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_new().
    unsafe { polyplug_runtime_free(rt) };
}
