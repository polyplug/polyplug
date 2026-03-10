//! polyplugc — CLI code-generation tool for the polyplug platform.

pub(crate) mod error;
pub(crate) mod generators;
pub(crate) mod ir;
pub(crate) mod parser;
pub(crate) mod pack;

use std::collections::HashMap;
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

        /// Target language: rust, cpp, csharp, python, lua, js-quickjs, js-deno.
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

    /// Generates scaffold metadata for packaging (no build execution)
    Pack {
        /// Path to the api.toml file
        #[arg(short, long)]
        api: Option<PathBuf>,
        /// Path to the bundle.toml file
        #[arg(short, long)]
        bundle: Option<PathBuf>,
        /// Target language (rust, cpp, csharp, python, lua, js-quickjs, js-deno)
        #[arg(short, long)]
        lang: String,
        /// Output directory for scaffold files
        #[arg(short, long)]
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
                "python" | "py" => Box::new(generators::python::PythonGenerator),
                "lua" => Box::new(generators::lua::LuaGenerator),
                "js-quickjs" => Box::new(generators::js_quickjs::JsQuickjsGenerator),
                "js-deno" => Box::new(generators::js_deno::JsDenoGenerator),
                other => {
                    return Err(CodegenError::ValidationFailed {
                        message: format!(
                            "Unknown language: `{other}`. Supported: rust, cpp, csharp, python, lua, js-quickjs, js-deno"
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

        Command::Pack {
            api,
            bundle,
            lang,
            out,
        } => {
            let ir: crate::ir::ValidatedIr = if let Some(api_path) = api {
                parser::parse_api(&api_path)?
            } else if let Some(bundle_path) = bundle {
                parser::parse_bundle_with_api(&bundle_path)?
            } else {
                return Err(CodegenError::ValidationFailed {
                    message: "Must specify --api or --bundle".to_owned(),
                });
            };
            pack::run(&ir, &out, &lang)?;
        }
    }
    Ok(())
}

/// FNV-1a 64-bit hash — no allocations, no fallibility.
fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037_u64;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211_u64);
    }
    hash
}

/// Write generated files to disk using FNV-1a hash cache for incremental generation.
/// Files with `force_regenerate == true` are always written (manifest.toml).
/// Prints: "regenerated N files, skipped M unchanged".
fn write_files_if_changed(out_dir: &Path, files: &GeneratedFiles) -> Result<(), CodegenError> {
    let cache_dir: PathBuf = out_dir.join(".polyplugc-cache");
    let cache_path: PathBuf = cache_dir.join("hashes.toml");

    // Load existing cache or start fresh.
    let mut cache: HashMap<String, u64> = if cache_path.exists() {
        let raw: String = std::fs::read_to_string(&cache_path).map_err(|e: std::io::Error| {
            CodegenError::CacheReadFailed {
                path: cache_path.to_string_lossy().into_owned(),
                source: e,
            }
        })?;
        toml::from_str::<HashMap<String, u64>>(&raw).map_err(|e: toml::de::Error| {
            CodegenError::CacheDeserializeFailed {
                path: cache_path.to_string_lossy().into_owned(),
                source: e,
            }
        })?
    } else {
        HashMap::new()
    };

    let mut regenerated: u32 = 0_u32;
    let mut skipped: u32 = 0_u32;

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

        let cache_key: String = file.path.display().to_string();

        if file.force_regenerate {
            // Always write manifest files; update cache.
            std::fs::write(&dest, &file.content).map_err(|e: std::io::Error| {
                CodegenError::WriteFailed {
                    path: dest.to_string_lossy().into_owned(),
                    source: e,
                }
            })?;
            let hash: u64 = fnv1a_64(file.content.as_bytes());
            cache.insert(cache_key, hash);
            regenerated += 1;
        } else {
            let hash: u64 = fnv1a_64(file.content.as_bytes());
            let cached_hash: Option<&u64> = cache.get(&cache_key);
            let hash_matches: bool = cached_hash == Some(&hash);
            if hash_matches && dest.exists() {
                // Identical content in cache and file exists — skip.
                skipped += 1;
            } else {
                // New or changed — write and update cache.
                std::fs::write(&dest, &file.content).map_err(|e: std::io::Error| {
                    CodegenError::WriteFailed {
                        path: dest.to_string_lossy().into_owned(),
                        source: e,
                    }
                })?;
                cache.insert(cache_key, hash);
                regenerated += 1;
            }
        }
    }

    // Save updated cache.
    std::fs::create_dir_all(&cache_dir).map_err(|e: std::io::Error| {
        CodegenError::CacheWriteFailed {
            path: cache_dir.to_string_lossy().into_owned(),
            source: e,
        }
    })?;
    let cache_toml: String = toml::to_string(&cache).map_err(|e: toml::ser::Error| {
        CodegenError::CacheSerializeFailed { source: e }
    })?;
    std::fs::write(&cache_path, cache_toml).map_err(|e: std::io::Error| {
        CodegenError::CacheWriteFailed {
            path: cache_path.to_string_lossy().into_owned(),
            source: e,
        }
    })?;

    println!("regenerated {regenerated} files, skipped {skipped} unchanged");
    Ok(())
}
