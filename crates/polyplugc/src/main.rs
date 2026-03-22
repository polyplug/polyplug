use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

use polyplug_codegen::{generate, pack, GenerateConfig, Lang, PackConfig, Side};

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

    /// Validate an api.toml or bundle.toml without generating code.
    Validate {
        /// Path to api.toml to validate.
        #[arg(long, conflicts_with = "bundle")]
        api: Option<PathBuf>,

        /// Path to bundle.toml to validate.
        #[arg(long, conflicts_with = "api")]
        bundle: Option<PathBuf>,
    },

    /// Generates scaffold metadata for packaging (no build execution)
    Pack {
        /// Path to the api.toml file
        #[arg(short, long)]
        api: Option<PathBuf>,
        /// Path to the bundle.toml file
        #[arg(short, long)]
        bundle: Option<PathBuf>,
        /// Target language (rust, cpp, csharp, python, lua, js-quickjs)
        #[arg(short, long)]
        lang: String,
        /// Output directory for scaffold files
        #[arg(short, long, required = true)]
        out: PathBuf,
    },
}

fn main() {
    let cli: Cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), polyplug_codegen::PolyplugcError> {
    match cli.command {
        Command::Generate {
            api,
            bundle,
            lang,
            out,
        } => {
            let (manifest, side) = if let Some(api_path) = api {
                (api_path, Side::Host)
            } else if let Some(bundle_path) = bundle {
                (bundle_path, Side::Guest)
            } else {
                return Err(polyplug_codegen::PolyplugcError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                });
            };

            let lang_enum = parse_lang(&lang)?;

            let config = GenerateConfig {
                api_toml: manifest,
                lang: lang_enum,
                side,
                out_dir: out.clone(),
            };

            let output = generate(config)?;

            // Write generated files
            write_files(&output, &out)?;
        }

        Command::Validate { api, bundle } => {
            let manifest = api.or(bundle).ok_or_else(|| {
                polyplug_codegen::PolyplugcError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                }
            })?;

            // Just parse to validate
            if manifest.ends_with("bundle.toml") {
                polyplug_codegen::parser::parse_bundle_with_api(&manifest)?;
            } else {
                polyplug_codegen::parser::parse_api(&manifest)?;
            }
            println!("OK: {}", manifest.display());
        }

        Command::Pack {
            api,
            bundle,
            lang,
            out,
        } => {
            let manifest = api.or(bundle).ok_or_else(|| {
                polyplug_codegen::PolyplugcError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                }
            })?;

            let lang_enum = parse_lang(&lang)?;

            let config = PackConfig {
                manifest,
                lang: lang_enum,
                out_dir: out,
            };

            pack(config)?;
        }
    }
    Ok(())
}

fn parse_lang(lang: &str) -> Result<Lang, polyplug_codegen::PolyplugcError> {
    match lang {
        "rust" => Ok(Lang::Rust),
        "cpp" | "c++" => Ok(Lang::Cpp),
        "csharp" | "c#" => Ok(Lang::CSharp),
        "python" | "py" => Ok(Lang::Python),
        "lua" => Ok(Lang::Lua),
        "js-quickjs" => Ok(Lang::JsQuickJs),
        other => Err(polyplug_codegen::PolyplugcError::ValidationFailed {
            message: format!(
                "Unknown language: `{other}`. Supported: rust, cpp, csharp, python, lua, js-quickjs"
            ),
        }),
    }
}

fn write_files(
    output: &polyplug_codegen::GenerateOutput,
    out_dir: &std::path::Path,
) -> Result<(), polyplug_codegen::PolyplugcError> {
    use std::fs;

    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                polyplug_codegen::PolyplugcError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source: e,
                }
            })?;
        }
        fs::write(&file_path, &file.content).map_err(|e| {
            polyplug_codegen::PolyplugcError::WriteFailed {
                path: file_path.to_string_lossy().into_owned(),
                source: e,
            }
        })?;
    }

    println!("generated {} files", output.files.len());
    Ok(())
}
