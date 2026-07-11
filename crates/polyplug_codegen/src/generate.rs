//! Public contract generation and output-writing pipeline.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Component;
use std::path::Path;
use std::process;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::Lang;
use crate::PolyplugcError;
use crate::RustGuestMode;
use crate::Side;

use crate::generators::CodeGenerator;
use crate::generators::cpp::CppGenerator;
use crate::generators::csharp::CSharpGenerator;
use crate::generators::js_quickjs::JsQuickjsGenerator;
use crate::generators::lua::LuaGenerator;
use crate::generators::python::PythonGenerator;
use crate::generators::rust::RustGenerator;
use crate::ir::ValidatedIr;
use crate::parser;

/// Generate contract bindings from an API or bundle manifest.
pub fn generate(config: GenerateConfig) -> Result<GenerateOutput, PolyplugcError> {
    let ir: ValidatedIr = parse_ir(&config)?;
    generate_ir(&ir, config.lang, config.side)
}

/// Generate Rust guest bindings in an explicitly selected linking mode.
///
/// The existing [`generate`] API always preserves disk-bundle output. Call this
/// function with [`RustGuestMode::InProcess`] to generate a runtime-local
/// `polyplug::InProcessBundle` constructor for a Rust guest.
pub fn generate_rust_guest(
    config: GenerateConfig,
    mode: RustGuestMode,
) -> Result<GenerateOutput, PolyplugcError> {
    if config.lang != Lang::Rust || config.side != Side::Guest {
        return Err(PolyplugcError::ValidationFailed {
            message: "Rust guest modes require Lang::Rust and Side::Guest".to_owned(),
        });
    }
    let ir: ValidatedIr = parse_ir(&config)?;
    generate_ir_rust_guest(&ir, mode)
}

/// Generate Rust guest bindings from validated IR in an explicitly selected
/// linking mode.
pub fn generate_ir_rust_guest(
    ir: &ValidatedIr,
    mode: RustGuestMode,
) -> Result<GenerateOutput, PolyplugcError> {
    let mut files: GenerateOutput = GenerateOutput::default();
    match mode {
        RustGuestMode::Disk => RustGenerator.generate_guest(ir, &mut files)?,
        RustGuestMode::InProcess { bundle_name } => {
            RustGenerator.generate_embedded_guest(ir, &bundle_name, &mut files)?
        }
    }
    Ok(files)
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

/// Generate contract bindings from validated IR.
pub fn generate_ir(
    ir: &ValidatedIr,
    lang: Lang,
    side: Side,
) -> Result<GenerateOutput, PolyplugcError> {
    let generator: Box<dyn CodeGenerator> = match lang {
        Lang::Rust => Box::new(RustGenerator),
        Lang::Cpp => Box::new(CppGenerator),
        Lang::CSharp => Box::new(CSharpGenerator),
        Lang::Python => Box::new(PythonGenerator),
        Lang::Lua => Box::new(LuaGenerator),
        Lang::JsQuickJs => Box::new(JsQuickjsGenerator),
    };

    let mut files: GenerateOutput = GenerateOutput::default();
    match side {
        Side::Host => generator.generate_host(ir, &mut files)?,
        Side::Guest => generator.generate_guest(ir, &mut files)?,
    }

    Ok(files)
}

/// Outcome of [`write_output`]: how many generated files were (re)written versus
/// skipped because their on-disk content already matched what would be emitted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WriteSummary {
    pub written: usize,
    pub unchanged: usize,
}

/// Write a [`GenerateOutput`] under `out_dir`.
///
/// Rust sources are formatted with `rustfmt` so the on-disk form is canonical, then
/// each file is written only when needed: a file with `force_regenerate` set (for
/// example `manifest.toml`, whose IDs must stay current) is always rewritten, and
/// any other file is rewritten only when its final content differs from what is
/// already on disk. Every generated file path must be relative and may not traverse
/// above `out_dir`.
pub fn write_output(
    output: &GenerateOutput,
    out_dir: &Path,
) -> Result<WriteSummary, PolyplugcError> {
    for file in &output.files {
        validate_output_path(&file.path)?;
    }

    let mut summary: WriteSummary = WriteSummary::default();
    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        let final_content: String = format_for_disk(&file_path, &file.content);

        if !file.force_regenerate {
            if let Ok(existing) = fs::read_to_string(&file_path) {
                if existing == final_content {
                    summary.unchanged += 1;
                    continue;
                }
            }
        }

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|source: io::Error| {
                PolyplugcError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source,
                }
            })?;
        }
        fs::write(&file_path, &final_content).map_err(|source: io::Error| {
            PolyplugcError::WriteFailed {
                path: file_path.to_string_lossy().into_owned(),
                source,
            }
        })?;
        summary.written += 1;
    }
    Ok(summary)
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
