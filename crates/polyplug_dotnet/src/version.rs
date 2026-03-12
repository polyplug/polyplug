//! version — pelite-based .NET assembly target framework reader.

use std::path::Path;

use pelite::PeFile;
use pelite::image::IMAGE_DATA_DIRECTORY;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;

/// COR20 header layout (ECMA-335 §II.25.3.3 / MSDN IMAGE_COR20_HEADER).
/// Used with pelite to locate the CLI metadata section.
#[derive(Clone, Copy)]
#[repr(C)]
struct ImageCor20Header {
    cb: u32,
    major_runtime_version: u16,
    minor_runtime_version: u16,
    metadata_rva: u32,
    metadata_size: u32,
    flags: u32,
    entry_point_token: u32,
    resources_rva: u32,
    resources_size: u32,
    strong_name_signature: u64,
    code_manager_table: u64,
    vtable_fixups: u64,
    extra_address_jumps: u64,
    managed_native_header: u64,
}

// SAFETY: ImageCor20Header is #[repr(C)] with no padding. All fields are explicitly
// sized primitive integer types (u32, u16, u64) with no pointer or reference members.
// The layout is guaranteed by the C ABI to be contiguous with natural alignment,
// matching the on-disk PE format defined by ECMA-335 §II.25.3.3.
// This type is Copy and has no destructor, so transmuting aligned bytes to &Self is sound.
unsafe impl pelite::Pod for ImageCor20Header {}

/// Index of the COM descriptor data directory entry in the PE optional header.
const IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR: usize = 14;

/// Marker byte pattern for the TargetFrameworkAttribute TFM string in CLI metadata.
const TFM_MARKER: &[u8] = b".NETCoreApp,Version=v";

/// Read the TargetFramework string from a .NET assembly DLL using pelite.
///
/// Returns the long-form TFM, e.g. `".NETCoreApp,Version=v10.0"`.
/// Returns `Ok(String::new())` for non-.NET DLLs or if no TFM attribute is found.
pub fn read_target_framework(dll_path: &Path) -> Result<String, PolyplugError> {
    // Step 1: Read file bytes.
    let bytes: Vec<u8> = std::fs::read(dll_path).map_err(|_| {
        PolyplugError::Loader(LoaderError::AssemblyNotFound {
            path: dll_path.to_string_lossy().into_owned(),
        })
    })?;

    // Step 2: Parse PE file — auto-detects PE32 vs PE32+.
    let pe: PeFile<'_> = PeFile::from_bytes(&bytes).map_err(|_| {
        PolyplugError::Loader(LoaderError::AssemblyNotFound {
            path: dll_path.to_string_lossy().into_owned(),
        })
    })?;

    // Step 3: Get COM descriptor data directory (index 14).
    let data_dirs: &[IMAGE_DATA_DIRECTORY] = pe.data_directory();
    if data_dirs.len() <= IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR {
        // No COM descriptor entry — not a .NET assembly.
        return Ok(String::new());
    }
    let com_dir: &IMAGE_DATA_DIRECTORY = &data_dirs[IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR];
    if com_dir.VirtualAddress == 0 {
        // Not a .NET assembly.
        return Ok(String::new());
    }

    // Step 4: Read COR20 header at the COM descriptor RVA.
    let cor20: &ImageCor20Header = pe.derva(com_dir.VirtualAddress).map_err(|_| {
        PolyplugError::Loader(LoaderError::AssemblyNotFound {
            path: dll_path.to_string_lossy().into_owned(),
        })
    })?;

    // Step 5: Get CLI metadata section as a byte slice.
    let metadata_slice: &[u8] = pe
        .derva_slice(cor20.metadata_rva, cor20.metadata_size as usize)
        .map_err(|_| {
            PolyplugError::Loader(LoaderError::AssemblyNotFound {
                path: dll_path.to_string_lossy().into_owned(),
            })
        })?;

    // Step 6: Scan for TFM marker in metadata bytes.
    // TargetFrameworkAttribute stores the TFM as a UTF-8 string in the #Blob heap.
    // The string ".NETCoreApp,Version=v" appears verbatim, consistent with the
    // existing sniff_target_framework implementation.
    let start: usize = match metadata_slice
        .windows(TFM_MARKER.len())
        .position(|w: &[u8]| w == TFM_MARKER)
    {
        Some(p) => p,
        None => return Ok(String::new()),
    };

    // Find end of string: first null byte after the marker start, or end of slice.
    let end: usize = metadata_slice[start..]
        .iter()
        .position(|&b: &u8| b == 0u8)
        .map(|n: usize| start + n)
        .unwrap_or(metadata_slice.len());

    let tfm_bytes: &[u8] = &metadata_slice[start..end];
    let tfm: String = String::from_utf8_lossy(tfm_bytes).into_owned();

    Ok(tfm) // e.g. ".NETCoreApp,Version=v10.0"
}
