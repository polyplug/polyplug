pub mod generators;
pub mod ir;
pub mod parser;
pub mod validate;

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::ir::ValidatedIr;
use generators::{CodeGenerator, GeneratedFile as InternalGeneratedFile, GeneratedFiles};
use polyplug_codegen::{GenerateConfig, GenerateOutput, Lang, PolyplugcError, Side};

pub fn generate(config: GenerateConfig) -> Result<GenerateOutput, PolyplugcError> {
    let file_content: String =
        fs::read_to_string(&config.api_toml).map_err(|e: std::io::Error| {
            PolyplugcError::ReadFailed {
                path: config.api_toml.to_string_lossy().to_string(),
                source: e,
            }
        })?;
    let ir: ValidatedIr = if file_content.contains("[bundle]") {
        parser::parse_bundle_with_api(&config.api_toml)?
    } else {
        parser::parse_api(&config.api_toml)?
    };

    let generator: Box<dyn CodeGenerator> = match config.lang {
        Lang::Rust => Box::new(generators::rust::RustGenerator),
        Lang::Cpp => Box::new(generators::cpp::CppGenerator),
        Lang::CSharp => Box::new(generators::csharp::CSharpGenerator),
        Lang::Python => Box::new(generators::python::PythonGenerator),
        Lang::Lua => Box::new(generators::lua::LuaGenerator),
        Lang::JsQuickJs => Box::new(generators::js_quickjs::JsQuickjsGenerator),
    };

    let mut files: GeneratedFiles = GeneratedFiles::default();
    match config.side {
        Side::Host => generator.generate_host(&ir, &mut files)?,
        Side::Guest => generator.generate_guest(&ir, &mut files)?,
    }

    let public_files: Vec<polyplug_codegen::GeneratedFile> = files
        .files
        .into_iter()
        .map(|f: InternalGeneratedFile| polyplug_codegen::GeneratedFile {
            path: f.path,
            content: f.content,
            force_regenerate: f.force_regenerate,
        })
        .collect();

    Ok(GenerateOutput {
        files: public_files,
    })
}

/// Outcome of [`write_output`]: how many generated files were (re)written versus
/// skipped because their on-disk content already matched what would be emitted.
#[derive(Debug, Default, Clone, Copy)]
pub struct WriteSummary {
    pub written: usize,
    pub unchanged: usize,
}

/// Write a [`GenerateOutput`] under `out_dir`.
///
/// Rust sources are formatted with `rustfmt` so the on-disk form is canonical, then
/// each file is written only when needed: a file with `force_regenerate` set (e.g.
/// `manifest.toml`, whose ids must stay current) is always rewritten, and any other
/// file is rewritten only when its final content differs from what is already on
/// disk. Skipping unchanged bindings preserves their mtimes so a no-op regeneration
/// does not cascade downstream rebuilds.
pub fn write_output(
    output: &GenerateOutput,
    out_dir: &Path,
) -> Result<WriteSummary, PolyplugcError> {
    let mut summary: WriteSummary = WriteSummary::default();
    for file in &output.files {
        let file_path: std::path::PathBuf = out_dir.join(&file.path);
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
            fs::create_dir_all(parent).map_err(|e: std::io::Error| {
                PolyplugcError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source: e,
                }
            })?;
        }
        fs::write(&file_path, &final_content).map_err(|e: std::io::Error| {
            PolyplugcError::WriteFailed {
                path: file_path.to_string_lossy().into_owned(),
                source: e,
            }
        })?;
        summary.written += 1;
    }
    Ok(summary)
}

/// Return `content` as it should land on disk: Rust sources are passed through
/// `rustfmt` (edition 2024) so the written bytes are canonical and a later byte
/// comparison against a re-generation is stable. rustfmt is best-effort — if it is
/// absent or exits non-zero (e.g. generated code cargo will reject anyway), the
/// unformatted source is returned unchanged.
fn format_for_disk(path: &Path, content: &str) -> String {
    let is_rust: bool = path.extension().and_then(|e: &std::ffi::OsStr| e.to_str()) == Some("rs");
    if !is_rust {
        return content.to_owned();
    }
    rustfmt_stdin(content).unwrap_or_else(|| content.to_owned())
}

/// Run `rustfmt`, feeding `content` on stdin and returning its formatted stdout, or
/// `None` if rustfmt is unavailable or exits non-zero. stdin is closed before the
/// output is collected so rustfmt observes EOF and cannot deadlock.
fn rustfmt_stdin(content: &str) -> Option<String> {
    let mut child: std::process::Child = std::process::Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin: std::process::ChildStdin = child.stdin.take()?;
    stdin.write_all(content.as_bytes()).ok()?;
    drop(stdin);
    let output: std::process::Output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

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
