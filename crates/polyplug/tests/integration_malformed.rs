//! Integration tests: malformed bundle inputs must return clean Err, never panic.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug_abi::HostApi;
use polyplug_utils::bundle_id;

fn load_bundle_path(host: *const HostApi, dir: &str) -> polyplug_abi::AbiError {
    let bytes: &[u8] = dir.as_bytes();
    // SAFETY: host non-null (checked by caller), bytes valid for bytes.len().
    unsafe { ((*host).load_bundle)(host, bytes.as_ptr(), bytes.len()) }
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
        "id = {}\nname = \"{}\"\nloader = \"{}\"\nfile = \"{}\"\n",
        bundle_id(name),
        name,
        runtime,
        file
    );
    fs::write(dir.join("manifest.toml"), manifest_toml).expect("write manifest");
}

#[test]
fn test_truncated_so() {
    // SAFETY: polyplug_runtime_create(core::ptr::null()) has no preconditions.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    let dir: PathBuf = make_tmpdir("truncated");
    let mut so: Vec<u8> = vec![0x7f_u8, b'E', b'L', b'F'];
    so.extend_from_slice(&[0u8; 508]);
    fs::write(dir.join("libtruncated.so"), &so).expect("write truncated so");
    write_manifest(&dir, "truncated", "native", "libtruncated.so");
    let rc: polyplug_abi::AbiError = load_bundle_path(host, dir.to_str().expect("valid utf8 path"));
    assert_ne!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "truncated .so must produce error"
    );
    cleanup(&dir);
    // SAFETY: host was returned by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_wrong_magic_bytes() {
    // SAFETY: polyplug_runtime_create(core::ptr::null()) has no preconditions.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    let dir: PathBuf = make_tmpdir("wrong_magic");
    let garbage: Vec<u8> = b"NOTANELF\x00".iter().cycle().take(512).cloned().collect();
    fs::write(dir.join("libwrong.so"), &garbage).expect("write garbage");
    write_manifest(&dir, "wrong_magic", "native", "libwrong.so");
    let rc: polyplug_abi::AbiError = load_bundle_path(host, dir.to_str().expect("valid utf8"));
    assert_ne!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "wrong magic bytes must produce error"
    );
    cleanup(&dir);
    // SAFETY: host was returned by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_missing_init_symbol() {
    let dir: &str = env!("NO_INIT_PLUGIN_DIR");
    // SAFETY: polyplug_runtime_create(core::ptr::null()) has no preconditions.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    let rc: polyplug_abi::AbiError = load_bundle_path(host, dir);
    assert_ne!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "plugin missing polyplug_init must produce error"
    );
    let mut buf: [u8; 256] = [0u8; 256];
    // SAFETY: buf valid for 256 bytes, host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };
    let msg: &str = core::str::from_utf8(&buf[..n]).expect("last_error is valid utf8");
    assert!(
        msg.contains("polyplug_init") || msg.contains("symbol") || msg.contains("init"),
        "error message should mention missing symbol, got: {}",
        msg
    );
    // SAFETY: host was returned by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_so_file_missing_from_bundle() {
    // SAFETY: polyplug_runtime_create(core::ptr::null()) has no preconditions.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    let dir: PathBuf = make_tmpdir("missing_so");
    write_manifest(&dir, "missing_so", "native", "nonexistent.so");
    let rc: polyplug_abi::AbiError = load_bundle_path(host, dir.to_str().expect("valid utf8"));
    assert_ne!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "missing .so file must produce error"
    );
    cleanup(&dir);
    // SAFETY: host was returned by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_unknown_runtime() {
    // SAFETY: polyplug_runtime_create(core::ptr::null()) has no preconditions.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    let dir: PathBuf = make_tmpdir("unknown_runtime");
    fs::write(dir.join("dummy.so"), b"notareal").expect("write dummy");
    write_manifest(&dir, "unknown_runtime", "cobol", "dummy.so");
    let rc: polyplug_abi::AbiError = load_bundle_path(host, dir.to_str().expect("valid utf8"));
    assert_ne!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "unknown runtime must produce error"
    );
    cleanup(&dir);
    // SAFETY: host was returned by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}
