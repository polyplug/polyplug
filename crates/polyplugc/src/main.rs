mod generators;
mod ir;
mod pack;
mod parser;

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

use generators::{
    CodeGenerator, GeneratedFile as InternalGeneratedFile, GeneratedFiles,
};
use polyplug_codegen::{
    GenerateConfig, GenerateOutput, Lang, PolyplugcError, Side,
};

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

fn run(cli: Cli) -> Result<(), PolyplugcError> {
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
                return Err(PolyplugcError::ValidationFailed {
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
                PolyplugcError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                }
            })?;

            // Just parse to validate
            if manifest.ends_with("bundle.toml") {
                parser::parse_bundle_with_api(&manifest)?;
            } else {
                parser::parse_api(&manifest)?;
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
                PolyplugcError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                }
            })?;

            let lang_enum = parse_lang(&lang)?;

            let ir: crate::ir::ValidatedIr = if manifest.ends_with("bundle.toml") {
                parser::parse_bundle_with_api(&manifest)?
            } else {
                parser::parse_api(&manifest)?
            };

            pack::run(&ir, &out, lang_enum.as_str())?;
        }
    }
    Ok(())
}

fn parse_lang(lang: &str) -> Result<Lang, PolyplugcError> {
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

fn generate(config: GenerateConfig) -> Result<GenerateOutput, PolyplugcError> {
    let file_content: String =
        fs::read_to_string(&config.api_toml).map_err(|e: std::io::Error| {
            PolyplugcError::ReadFailed {
                path: config.api_toml.to_string_lossy().to_string(),
                source: e,
            }
        })?;
    let ir: crate::ir::ValidatedIr = if file_content.contains("[bundle]") {
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
        })
        .collect();

    Ok(GenerateOutput {
        files: public_files,
    })
}

fn write_files(
    output: &GenerateOutput,
    out_dir: &std::path::Path,
) -> Result<(), PolyplugcError> {
    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                PolyplugcError::WriteFailed {
                    path: parent.to_string_lossy().into_owned(),
                    source: e,
                }
            })?;
        }
        fs::write(&file_path, &file.content).map_err(|e| {
            PolyplugcError::WriteFailed {
                path: file_path.to_string_lossy().into_owned(),
                source: e,
            }
        })?;
    }

    println!("generated {} files", output.files.len());
    Ok(())
}
