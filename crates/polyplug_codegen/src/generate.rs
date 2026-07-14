//! Public contract generation and output-writing pipeline.

#[cfg(test)]
use core::cell::Cell;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashSet;
use std::env::current_dir;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;
#[cfg(windows)]
use std::iter::once;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::GeneratedFile;
use crate::InternalCSharpGenerateConfig;
use crate::InternalCppGenerateConfig;
use crate::InternalJavaScriptGenerateConfig;
use crate::InternalLuaGenerateConfig;
use crate::InternalPythonGenerateConfig;
use crate::InternalRustGenerateConfig;
use crate::Lang;
use crate::OutputDestination;
use crate::OutputLayout;
use crate::PolyplugcError;
use crate::Side;

use crate::generators::CodeGenerator;
use crate::generators::cpp::CppGenerator;
use crate::generators::csharp::CSharpGenerator;
use crate::generators::js_quickjs::JsQuickjsGenerator;
use crate::generators::lua::LuaGenerator;
use crate::generators::python::PythonGenerator;
use crate::generators::rust::{RustGenerator, apply_rust_layout_imports};
use crate::ir::ValidatedIr;
use crate::parser;

/// Generate contract bindings under the configuration's canonical semantic layout.
pub fn generate(config: GenerateConfig) -> Result<GenerateOutput, PolyplugcError> {
    let ir: ValidatedIr = parse_ir(&config)?;
    let output = generate_ir_with_layout(&ir, config.lang, config.side, config.layout.clone())?;
    output.layout().validate(config.lang, &output.files)?;
    Ok(output)
}

/// Generate the opt-in C++ internal-plugin profile for one bundle.
pub fn generate_internal_cpp(
    config: InternalCppGenerateConfig,
) -> Result<GenerateOutput, PolyplugcError> {
    generate_internal_profile(
        &config.bundle_toml,
        "C++",
        Lang::Cpp,
        &config.layout,
        |ir, bundle_name, layout, output| {
            CppGenerator.generate_internal_bundle(ir, bundle_name, layout, output)
        },
    )
}

/// Generate the opt-in C# internal-plugin profile for one bundle.
pub fn generate_internal_csharp(
    config: InternalCSharpGenerateConfig,
) -> Result<GenerateOutput, PolyplugcError> {
    generate_internal_profile(
        &config.bundle_toml,
        "C#",
        Lang::CSharp,
        &config.layout,
        |ir, bundle_name, layout, output| {
            CSharpGenerator.generate_internal_bundle(ir, bundle_name, layout, output)
        },
    )
}

/// Generate the opt-in JavaScript internal-plugin profile for one bundle.
pub fn generate_internal_javascript(
    config: InternalJavaScriptGenerateConfig,
) -> Result<GenerateOutput, PolyplugcError> {
    generate_internal_profile(
        &config.bundle_toml,
        "JavaScript",
        Lang::JsQuickJs,
        &config.layout,
        |ir, bundle_name, layout, output| {
            JsQuickjsGenerator.generate_internal_bundle(ir, bundle_name, layout, output)
        },
    )
}

/// Generate the opt-in Lua internal-plugin profile for one bundle.
pub fn generate_internal_lua(
    config: InternalLuaGenerateConfig,
) -> Result<GenerateOutput, PolyplugcError> {
    generate_internal_profile(
        &config.bundle_toml,
        "Lua",
        Lang::Lua,
        &config.layout,
        |ir, bundle_name, layout, output| {
            LuaGenerator.generate_internal_bundle(ir, bundle_name, layout, output)
        },
    )
}

/// Generate the opt-in Python internal-plugin profile for one bundle.
pub fn generate_internal_python(
    config: InternalPythonGenerateConfig,
) -> Result<GenerateOutput, PolyplugcError> {
    generate_internal_profile(
        &config.bundle_toml,
        "Python",
        Lang::Python,
        &config.layout,
        |ir, bundle_name, layout, output| {
            PythonGenerator.generate_internal_bundle(ir, bundle_name, layout, output)
        },
    )
}

pub fn generate_internal_rust(
    config: InternalRustGenerateConfig,
) -> Result<GenerateOutput, PolyplugcError> {
    let mut output = generate_internal_profile(
        &config.bundle_toml,
        "Rust",
        Lang::Rust,
        &config.layout,
        |ir, bundle_name, layout, output| {
            RustGenerator.generate_internal_bundle(ir, bundle_name, layout, output)
        },
    )?;
    let layout = output.layout().clone();
    apply_rust_layout_imports(&mut output, &layout);
    output.layout().validate(Lang::Rust, &output.files)?;
    Ok(output)
}

fn generate_internal_profile(
    bundle_toml: &Path,
    language: &str,
    lang: Lang,
    layout: &OutputLayout,
    generate: impl FnOnce(
        &ValidatedIr,
        &str,
        &OutputLayout,
        &mut GenerateOutput,
    ) -> Result<(), PolyplugcError>,
) -> Result<GenerateOutput, PolyplugcError> {
    let ir: ValidatedIr = parser::parse_bundle_with_api_internal(bundle_toml)?;
    let bundle = ir
        .bundle
        .as_ref()
        .ok_or_else(|| PolyplugcError::ValidationFailed {
            message: format!("internal {language} generation requires a bundle manifest"),
        })?;
    let mut output = GenerateOutput::new(lang, layout.clone());
    generate(&ir, &bundle.name, layout, &mut output)?;
    prefix_internal_output(&mut output, &bundle.name, bundle.bundle_id);
    reject_duplicate_output_paths(&output)?;
    output.layout().validate(lang, &output.files)?;
    Ok(output)
}

fn parse_ir(config: &GenerateConfig) -> Result<ValidatedIr, PolyplugcError> {
    let file_content: String =
        fs::read_to_string(&config.api_toml).map_err(|source: io::Error| {
            PolyplugcError::ReadFailed {
                path: config.api_toml.to_string_lossy().into_owned(),
                source,
            }
        })?;
    if file_content.contains("[bundle]") {
        parser::parse_bundle_with_api(&config.api_toml)
    } else {
        parser::parse_api(&config.api_toml)
    }
}

#[cfg(test)]
/// Generate contract bindings from validated IR for internal test helpers.
pub(crate) fn generate_ir(
    ir: &ValidatedIr,
    lang: Lang,
    side: Side,
) -> Result<GenerateOutput, PolyplugcError> {
    generate_ir_with_layout(ir, lang, side, OutputLayout::unified())
}

fn generate_ir_with_layout(
    ir: &ValidatedIr,
    lang: Lang,
    side: Side,
    layout: OutputLayout,
) -> Result<GenerateOutput, PolyplugcError> {
    let generator: Box<dyn CodeGenerator> = match lang {
        Lang::Rust => Box::new(RustGenerator),
        Lang::Cpp => Box::new(CppGenerator),
        Lang::CSharp => Box::new(CSharpGenerator),
        Lang::Python => Box::new(PythonGenerator),
        Lang::Lua => Box::new(LuaGenerator),
        Lang::JsQuickJs => Box::new(JsQuickjsGenerator),
    };

    let mut files = GenerateOutput::new(lang, layout.clone());
    match side {
        Side::Host => generator.generate_host(ir, &layout, &mut files)?,
        Side::Guest => generator.generate_guest(ir, &layout, &mut files)?,
    }
    generator.apply_output_layout(ir, side, &layout, &mut files)?;
    Ok(files)
}

fn prefix_internal_output(output: &mut GenerateOutput, bundle_name: &str, bundle_id: u64) {
    let readable: String = bundle_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect();
    let prefix: PathBuf = Path::new("internal").join(format!("{readable}-{bundle_id:016x}"));
    for file in &mut output.files {
        file.path = prefix.join(&file.path);
    }
}

fn reject_duplicate_output_paths(output: &GenerateOutput) -> Result<(), PolyplugcError> {
    let mut paths: HashSet<&Path> = HashSet::with_capacity(output.files.len());
    for file in &output.files {
        if !paths.insert(file.path.as_path()) {
            return Err(PolyplugcError::DuplicateOutputPath {
                path: file.path.to_string_lossy().into_owned(),
            });
        }
    }
    Ok(())
}

/// Outcome of [`write_output`]: how many generated files were (re)written versus
/// skipped because their on-disk content already matched what would be emitted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WriteSummary {
    pub written: usize,
    pub unchanged: usize,
}

struct OutputTarget<'a> {
    file: &'a GeneratedFile,
    path: PathBuf,
    identity: PathBuf,
}

/// Preflight and write the output's own canonical layout.
pub fn write_output(
    output: &GenerateOutput,
    out_dir: &Path,
) -> Result<WriteSummary, PolyplugcError> {
    output.layout().validate(output.language(), &output.files)?;
    write_output_with_layout(output, out_dir, output.layout())
}

/// Preflight and write generated files according to their semantic destinations.
///
/// The complete target set is validated before any directory is created. Files are
/// formatted before comparison so an unchanged file retains its mtime, and changed
/// files are committed through a same-directory temporary path and rename.
fn write_output_with_layout(
    output: &GenerateOutput,
    out_dir: &Path,
    layout: &OutputLayout,
) -> Result<WriteSummary, PolyplugcError> {
    layout.validate_references(&output.files)?;
    let mut targets: Vec<OutputTarget<'_>> = Vec::new();
    let mut paths: HashSet<PathBuf> = HashSet::with_capacity(output.files.len());
    for file in &output.files {
        validate_output_path(&file.path)?;
        let destination: &OutputDestination = layout.destination(file.partition);
        let root: &Path = match destination {
            OutputDestination::Inline => out_dir,
            OutputDestination::Emit { root, .. } => root,
            OutputDestination::ImportOnly { .. } | OutputDestination::Omit => continue,
        };
        let path: PathBuf = root.join(&file.path);
        let identity: PathBuf = normalized_target_identity(&path)?;
        if !paths.insert(identity.clone()) {
            return Err(PolyplugcError::DuplicateOutputPath {
                path: identity.to_string_lossy().into_owned(),
            });
        }
        targets.push(OutputTarget {
            file,
            path,
            identity,
        });
    }
    preflight_targets(&targets)?;

    let mut summary: WriteSummary = WriteSummary::default();
    for target in targets {
        let final_content: String = format_for_disk(&target.path, &target.file.content);
        if !target.file.force_regenerate
            && fs::read_to_string(&target.path).is_ok_and(|existing| existing == final_content)
        {
            summary.unchanged += 1;
            continue;
        }
        if let Some(parent) = target.path.parent() {
            fs::create_dir_all(parent).map_err(|source: io::Error| {
                PolyplugcError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source,
                }
            })?;
        }
        atomic_write(&target.path, &final_content)?;
        summary.written += 1;
    }
    Ok(summary)
}

fn normalized_target_identity(path: &Path) -> Result<PathBuf, PolyplugcError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir()
            .map_err(|source| PolyplugcError::WriteFailed {
                path: path.to_string_lossy().into_owned(),
                source,
            })?
            .join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix: Vec<OsString> = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    let link_target =
                        fs::read_link(ancestor).map_err(|source| PolyplugcError::WriteFailed {
                            path: ancestor.to_string_lossy().into_owned(),
                            source,
                        })?;
                    if let Err(source) = ancestor.canonicalize() {
                        if source.kind() == io::ErrorKind::NotFound {
                            return Err(PolyplugcError::ValidationFailed {
                                message: format!(
                                    "generated output ancestor `{}` is a dangling symlink to `{}`",
                                    ancestor.display(),
                                    link_target.display()
                                ),
                            });
                        }
                        return Err(PolyplugcError::WriteFailed {
                            path: ancestor.to_string_lossy().into_owned(),
                            source,
                        });
                    }
                }
                break;
            }
            Err(source)
                if matches!(
                    source.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                let name =
                    ancestor
                        .file_name()
                        .ok_or_else(|| PolyplugcError::ValidationFailed {
                            message: format!(
                                "cannot resolve generated output root `{}`",
                                path.display()
                            ),
                        })?;
                suffix.push(name.to_os_string());
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| PolyplugcError::ValidationFailed {
                        message: format!(
                            "cannot resolve generated output root `{}`",
                            path.display()
                        ),
                    })?;
            }
            Err(source) => {
                return Err(PolyplugcError::WriteFailed {
                    path: ancestor.to_string_lossy().into_owned(),
                    source,
                });
            }
        }
    }
    let mut normalized = ancestor
        .canonicalize()
        .map_err(|source| PolyplugcError::WriteFailed {
            path: ancestor.to_string_lossy().into_owned(),
            source,
        })?;
    for component in suffix.iter().rev() {
        normalized.push(component);
    }
    Ok(normalize_target_case(normalized))
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn normalize_target_case(path: PathBuf) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn normalize_target_case(path: PathBuf) -> PathBuf {
    path
}

struct TemporaryOutputCleanup<'a> {
    path: &'a Path,
    active: bool,
}

impl<'a> TemporaryOutputCleanup<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TemporaryOutputCleanup<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_file(self.path);
        }
    }
}

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum AtomicWriteFailure {
    Write,
    Replace,
}

#[cfg(test)]
thread_local! {
    static FORCE_ATOMIC_WRITE_FAILURE: Cell<Option<AtomicWriteFailure>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn force_next_atomic_write_failure() {
    FORCE_ATOMIC_WRITE_FAILURE.with(|force| force.set(Some(AtomicWriteFailure::Write)));
}

#[cfg(test)]
pub(crate) fn force_next_atomic_replace_failure() {
    FORCE_ATOMIC_WRITE_FAILURE.with(|force| force.set(Some(AtomicWriteFailure::Replace)));
}

#[cfg(test)]
fn take_atomic_write_failure() -> Option<AtomicWriteFailure> {
    FORCE_ATOMIC_WRITE_FAILURE.with(Cell::take)
}

fn preflight_targets(targets: &[OutputTarget<'_>]) -> Result<(), PolyplugcError> {
    for target in targets {
        if target.path.is_dir() {
            return Err(PolyplugcError::ValidationFailed {
                message: format!(
                    "generated file target `{}` is an existing directory",
                    target.path.display()
                ),
            });
        }
        let mut ancestor = target.path.parent();
        while let Some(path) = ancestor {
            if path.exists() && !path.is_dir() {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "generated output ancestor `{}` is not a directory",
                        path.display()
                    ),
                });
            }
            ancestor = path.parent();
        }
    }
    for child in targets {
        for parent in targets {
            if child.identity != parent.identity && child.identity.starts_with(&parent.identity) {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "generated file target `{}` conflicts with descendant `{}`",
                        parent.path.display(),
                        child.path.display()
                    ),
                });
            }
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<(), PolyplugcError> {
    let parent: &Path = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name: &OsStr = path.file_name().unwrap_or_else(|| OsStr::new("generated"));
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp: PathBuf = parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        process::id(),
        sequence
    ));
    let mut cleanup = TemporaryOutputCleanup::new(&temp);
    let mut temporary =
        File::create(&temp).map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: temp.to_string_lossy().into_owned(),
            source,
        })?;
    #[cfg(test)]
    let failure = take_atomic_write_failure();
    #[cfg(test)]
    if failure == Some(AtomicWriteFailure::Write) {
        return Err(PolyplugcError::WriteFailed {
            path: temp.to_string_lossy().into_owned(),
            source: io::Error::other("injected atomic write failure"),
        });
    }
    temporary
        .write_all(content.as_bytes())
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: temp.to_string_lossy().into_owned(),
            source,
        })?;
    drop(temporary);
    #[cfg(test)]
    let replace_result = if failure == Some(AtomicWriteFailure::Replace) {
        Err(io::Error::other("injected atomic replace failure"))
    } else {
        replace_file(&temp, path)
    };
    #[cfg(not(test))]
    let replace_result = replace_file(&temp, path);
    replace_result.map_err(|source| PolyplugcError::WriteFailed {
        path: path.to_string_lossy().into_owned(),
        source,
    })?;
    cleanup.disarm();
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(temp: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp, destination)
}

#[cfg(windows)]
fn replace_file(temp: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    let temp_wide: Vec<u16> = temp.as_os_str().encode_wide().chain(once(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            temp_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn validate_output_path(path: &Path) -> Result<(), PolyplugcError> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(PolyplugcError::UnsafeOutputPath {
            path: path.to_string_lossy().into_owned(),
        });
    }

    Ok(())
}

/// Return `content` as it should land on disk. Rust sources are passed through
/// `rustfmt` (edition 2024) so the written bytes are canonical. If `rustfmt` is
/// unavailable or rejects generated input, the original content is written.
fn format_for_disk(path: &Path, content: &str) -> String {
    let is_rust: bool = path
        .extension()
        .and_then(|extension: &OsStr| extension.to_str())
        == Some("rs");
    if !is_rust {
        return content.to_owned();
    }
    rustfmt_stdin(content).unwrap_or_else(|| content.to_owned())
}

fn rustfmt_stdin(content: &str) -> Option<String> {
    let mut child: process::Child = process::Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin: process::ChildStdin = child.stdin.take()?;
    stdin.write_all(content.as_bytes()).ok()?;
    drop(stdin);
    let output: process::Output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Parse a CLI language spelling into [`Lang`].
pub fn parse_lang(lang: &str) -> Result<Lang, PolyplugcError> {
    match lang {
        "rust" => Ok(Lang::Rust),
        "cpp" | "c++" => Ok(Lang::Cpp),
        "csharp" | "c#" => Ok(Lang::CSharp),
        "python" | "py" => Ok(Lang::Python),
        "lua" => Ok(Lang::Lua),
        "js-quickjs" => Ok(Lang::JsQuickJs),
        other => Err(PolyplugcError::ValidationFailed {
            message: format!(
                "Unknown language: `{other}`. Supported: rust, cpp, csharp, python, lua, js-quickjs"
            ),
        }),
    }
}
