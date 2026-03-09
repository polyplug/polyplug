//! polyplugc — CLI code-generation tool for the polyplug platform.

pub(crate) mod error;
pub(crate) mod generators;
pub(crate) mod ir;
pub(crate) mod parser;

use std::path::Path;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

use crate::error::CodegenError;
use crate::generators::GeneratedFiles;

/// polyplugc — code generator for the polyplug plugin runtime.
#[derive(Debug, Parser)]
#[command(
    name = "polyplugc",
    about = "Generate polyplug plugin boilerplate",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate code from an api.toml or bundle.toml.
    Generate {
        /// Path to api.toml (generates host-side code).
        #[arg(long, conflicts_with = "bundle")]
        api: Option<PathBuf>,

        /// Path to bundle.toml (generates guest-side code).
        #[arg(long, conflicts_with = "api")]
        bundle: Option<PathBuf>,

        /// Target language: rust or cpp.
        #[arg(long, short = 'l')]
        lang: String,

        /// Output directory for generated files.
        #[arg(long, short = 'o')]
        out: PathBuf,
    },

    /// Validate an api.toml or bundle.toml without generating code.
    Validate {
        /// Path to api.toml to validate.
        #[arg(long, conflicts_with = "bundle")]
        api: Option<PathBuf>,

        /// Path to bundle.toml to validate.
        #[arg(long, conflicts_with = "api")]
        bundle: Option<PathBuf>,
    },
}

fn main() {
    let cli: Cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), CodegenError> {
    match cli.command {
        Command::Generate {
            api,
            bundle,
            lang,
            out,
        } => {
            let from_api: bool = api.is_some();
            let ir: crate::ir::ValidatedIr = if let Some(api_path) = api {
                parser::parse_api(&api_path)?
            } else if let Some(bundle_path) = bundle {
                parser::parse_bundle_with_api(&bundle_path)?
            } else {
                return Err(CodegenError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                });
            };

            let generator: Box<dyn generators::CodeGenerator> = match lang.as_str() {
                "rust" => Box::new(generators::rust::RustGenerator),
                "cpp" | "c++" => Box::new(generators::cpp::CppGenerator),
                "csharp" | "c#" => Box::new(generators::csharp::CSharpGenerator),
                other => {
                    return Err(CodegenError::ValidationFailed {
                        message: format!(
                            "Unknown language: `{other}`. Supported: rust, cpp, csharp"
                        ),
                    });
                }
            };

            let mut files: GeneratedFiles = GeneratedFiles::default();
            if from_api {
                generator.generate_host(&ir, &mut files)?;
                generator.generate_guest(&ir, &mut files)?;
            } else {
                generator.generate_guest(&ir, &mut files)?;
            }

            // Create output directory if it doesn't exist
            std::fs::create_dir_all(&out).map_err(|e| CodegenError::WriteFailed {
                path: out.to_string_lossy().into_owned(),
                source: e,
            })?;

            // Write generated files
            write_files_if_changed(&out, &files)?;
        }

        Command::Validate { api, bundle } => {
            if let Some(api_path) = api {
                parser::parse_api(&api_path)?;
                println!("OK: {}", api_path.display());
            } else if let Some(bundle_path) = bundle {
                parser::parse_bundle_with_api(&bundle_path)?;
                println!("OK: {}", bundle_path.display());
            } else {
                return Err(CodegenError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// Write generated files to disk, only updating files that changed (idempotent).
fn write_files_if_changed(out_dir: &Path, files: &GeneratedFiles) -> Result<(), CodegenError> {
    for file in &files.files {
        let dest: PathBuf = out_dir.join(&file.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e: std::io::Error| {
                CodegenError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source: e,
                }
            })?;
        }
        // Check if file exists and content is identical (avoid unnecessary writes)
        let needs_write: bool = match std::fs::read_to_string(&dest) {
            Ok(existing) => existing != file.content,
            Err(_) => true, // File doesn't exist or can't be read
        };
        if needs_write {
            std::fs::write(&dest, &file.content).map_err(|e| CodegenError::WriteFailed {
                path: dest.to_string_lossy().into_owned(),
                source: e,
            })?;
        }
    }
    Ok(())
}
