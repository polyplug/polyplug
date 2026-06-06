use std::fs;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

use polyplug_codegen::{GenerateConfig, GenerateOutput, PolyplugcError, Side};
use polyplugc::{generate, parse_lang, parser, validate};

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

        /// Target language: rust, cpp, csharp, python, lua, js-quickjs.
        #[arg(long, short = 'l')]
        lang: String,

        /// Output directory for generated files.
        #[arg(long, short = 'o', required = true)]
        out: PathBuf,
    },

    /// Validate an api.toml / bundle.toml, or an assembled bundle directory.
    Validate {
        /// Path to api.toml to validate.
        #[arg(long, conflicts_with_all = ["bundle", "bundle_dir"])]
        api: Option<PathBuf>,

        /// Path to bundle.toml to validate.
        #[arg(long, conflicts_with_all = ["api", "bundle_dir"])]
        bundle: Option<PathBuf>,

        /// Path to an assembled bundle directory (manifest.toml + entry artifact)
        /// to validate against the runtime loader's own manifest machinery.
        #[arg(long, conflicts_with_all = ["api", "bundle"])]
        bundle_dir: Option<PathBuf>,
    },
}

fn main() {
    let cli: Cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), PolyplugcError> {
    match cli.command {
        Command::Generate {
            api,
            bundle,
            lang,
            out,
        } => {
            let (manifest, side): (PathBuf, Side) = if let Some(api_path) = api {
                (api_path, Side::Host)
            } else if let Some(bundle_path) = bundle {
                (bundle_path, Side::Guest)
            } else {
                return Err(PolyplugcError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                });
            };

            let config: GenerateConfig = GenerateConfig {
                api_toml: manifest,
                lang: parse_lang(&lang)?,
                side,
                out_dir: out.clone(),
            };

            let output: GenerateOutput = generate(config)?;
            write_files(&output, &out)?;
        }

        Command::Validate {
            api,
            bundle,
            bundle_dir,
        } => {
            if let Some(dir) = bundle_dir {
                validate::validate_bundle_dir(&dir)?;
                println!("OK: {}", dir.display());
            } else {
                let manifest: PathBuf =
                    api.or(bundle)
                        .ok_or_else(|| PolyplugcError::ValidationFailed {
                            message: "Must specify --api, --bundle, or --bundle-dir".to_owned(),
                        })?;

                // Just parse to validate.
                if manifest.ends_with("bundle.toml") {
                    parser::parse_bundle_with_api(&manifest)?;
                } else {
                    parser::parse_api(&manifest)?;
                }
                println!("OK: {}", manifest.display());
            }
        }
    }
    Ok(())
}

fn write_files(output: &GenerateOutput, out_dir: &std::path::Path) -> Result<(), PolyplugcError> {
    for file in &output.files {
        let file_path: PathBuf = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e: std::io::Error| {
                PolyplugcError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source: e,
                }
            })?;
        }
        fs::write(&file_path, &file.content).map_err(|e: std::io::Error| {
            PolyplugcError::WriteFailed {
                path: file_path.to_string_lossy().into_owned(),
                source: e,
            }
        })?;

        // Format Rust source files with rustfmt so generated output is already canonical.
        // rustfmt is a best-effort post-pass: if it is absent or fails (e.g. syntax error
        // in generated code that cargo will catch later), we do not abort the write.
        if file_path
            .extension()
            .and_then(|e: &std::ffi::OsStr| e.to_str())
            == Some("rs")
        {
            let _ = std::process::Command::new("rustfmt")
                .arg("--edition")
                .arg("2024")
                .arg(&file_path)
                .status();
        }
    }

    println!("generated {} files", output.files.len());
    Ok(())
}
