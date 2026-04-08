//! Integration tests: malformed bundle inputs must return clean Err, never panic.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::ffi::polyplug_runtime_last_error;
use polyplug::ffi::polyplug_runtime_load_bundle;

fn load_bundle_path(rt: *mut OpaqueRuntime, dir: &str) -> u32 {
    let bytes: &[u8] = dir.as_bytes();
    // SAFETY: rt non-null (checked by caller), bytes valid for bytes.len().
    unsafe { polyplug_runtime_load_bundle(rt, bytes.as_ptr(), bytes.len()) }
}

fn make_tmpdir(name: &str) -> PathBuf {
    let base: PathBuf = std::env::temp_dir().join(format!("polyplug_test_{name}"));
    fs::create_dir_all(&base).expect("create tmpdir");
    base
}

fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

fn write_manifest(dir: &Path, name: &str, runtime: &str, file: &str) {
    let manifest_toml: String = format!(
        "id = 1\nname = \"{}\"\nruntime = \"{}\"\nfile = \"{}\"\n",
        name, runtime, file
    );
    fs::write(dir.join("manifest.toml"), manifest_toml).expect("write manifest");
}

#[test]
fn test_truncated_so() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("truncated");
    let mut so: Vec<u8> = vec![0x7f_u8, b'E', b'L', b'F'];
    so.extend_from_slice(&[0u8; 508]);
    fs::write(dir.join("libtruncated.so"), &so).expect("write truncated so");
    write_manifest(&dir, "truncated", "native", "libtruncated.so");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8 path"));
    assert_ne!(rc, 0, "truncated .so must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_wrong_magic_bytes() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("wrong_magic");
    let garbage: Vec<u8> = b"NOTANELF\x00".iter().cycle().take(512).cloned().collect();
    fs::write(dir.join("libwrong.so"), &garbage).expect("write garbage");
    write_manifest(&dir, "wrong_magic", "native", "libwrong.so");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
    assert_ne!(rc, 0, "wrong magic bytes must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_missing_init_symbol() {
    let dir: &str = env!("NO_INIT_PLUGIN_DIR");
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let rc: u32 = load_bundle_path(rt, dir);
    assert_ne!(
        rc, 0,
        "plugin missing polyplug_init must produce non-zero return"
    );
    let mut buf: [u8; 256] = [0u8; 256];
    // SAFETY: buf valid for 256 bytes, polyplug_runtime_last_error writes at most buf_len bytes; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };
    let msg: &str = core::str::from_utf8(&buf[..n]).expect("last_error is valid utf8");
    assert!(
        msg.contains("polyplug_init") || msg.contains("symbol") || msg.contains("init"),
        "error message should mention missing symbol, got: {}",
        msg
    );
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_so_file_missing_from_bundle() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("missing_so");
    write_manifest(&dir, "missing_so", "native", "nonexistent.so");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
    assert_ne!(rc, 0, "missing .so file must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_unknown_runtime() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let dir: PathBuf = make_tmpdir("unknown_runtime");
    fs::write(dir.join("dummy.so"), b"notareal").expect("write dummy");
    write_manifest(&dir, "unknown_runtime", "cobol", "dummy.so");
    let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
    assert_ne!(rc, 0, "unknown runtime must produce non-zero return");
    cleanup(&dir);
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}