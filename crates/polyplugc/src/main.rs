use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use clap::Subcommand;

use ed25519_dalek::SigningKey;
use ed25519_dalek::VerifyingKey;
use polyplug_codegen::{GenerateConfig, GenerateOutput, PolyplugcError, Side};
use polyplug_signing::{
    SigError, VerifiedBundle, generate_keypair, load_signing_key, save_signing_key,
    save_verifying_key, sign_bundle, verify_bundle,
};
use polyplugc::{WriteSummary, generate, parse_lang, parser, validate, write_output};

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

    /// Generate an Ed25519 keypair for bundle signing.
    ///
    /// Writes two files into `--out`:
    ///   - `signing.key`   (private — keep secret, 0o600)
    ///   - `verifying.key` (public  — distribute alongside bundles or embed)
    Keygen {
        /// Directory to write the keypair files into.
        #[arg(long, short = 'o', required = true)]
        out: PathBuf,
    },

    /// Sign a bundle directory, writing a `bundle.sig` file.
    ///
    /// Runs the same validation checks as `validate --bundle-dir` first, then
    /// computes the canonical bundle digest and writes a detached signature.
    Sign {
        /// Path to the assembled bundle directory.
        #[arg(long, required = true)]
        bundle_dir: PathBuf,

        /// Path to the signing key file produced by `keygen`.
        #[arg(long, required = true)]
        key: PathBuf,
    },

    /// Verify a bundle directory's `bundle.sig`.
    ///
    /// Exits zero on success, non-zero on failure.
    Verify {
        /// Path to the assembled bundle directory.
        #[arg(long, required = true)]
        bundle_dir: PathBuf,
    },
}

fn main() {
    let cli: Cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        process::exit(1);
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

        Command::Keygen { out } => {
            fs::create_dir_all(&out).map_err(|e: io::Error| PolyplugcError::WriteFailed {
                path: out.display().to_string(),
                source: e,
            })?;

            let (signing_key, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();

            let signing_path: PathBuf = out.join("signing.key");
            let verifying_path: PathBuf = out.join("verifying.key");

            save_signing_key(&signing_path, &signing_key).map_err(sig_to_polyplugc_error)?;
            save_verifying_key(&verifying_path, &verifying_key).map_err(sig_to_polyplugc_error)?;

            println!("signing key:   {}", signing_path.display());
            println!("verifying key: {}", verifying_path.display());
        }

        Command::Sign { bundle_dir, key } => {
            // Run the same bundle-dir validation the `validate --bundle-dir` path does.
            validate::validate_bundle_dir(&bundle_dir)?;

            let signing_key: SigningKey = load_signing_key(&key).map_err(sig_to_polyplugc_error)?;
            sign_bundle(&bundle_dir, &signing_key).map_err(sig_to_polyplugc_error)?;

            println!("signed: {}", bundle_dir.join("bundle.sig").display());
        }

        Command::Verify { bundle_dir } => {
            let result: Result<VerifiedBundle, SigError> = verify_bundle(&bundle_dir);
            match result {
                Ok(verified) => {
                    println!("PASS: {}", verified.bundle_dir.display());
                    println!(
                        "verifying key: {}",
                        hex_encode(verified.verifying_key.as_bytes())
                    );
                }
                Err(e) => {
                    eprintln!("FAIL: {e}");
                    process::exit(1);
                }
            }
        }
    }
    Ok(())
}

/// Convert a [`SigError`] into a [`PolyplugcError::ValidationFailed`] for
/// uniform CLI error reporting.
fn sig_to_polyplugc_error(e: SigError) -> PolyplugcError {
    PolyplugcError::ValidationFailed {
        message: e.to_string(),
    }
}

/// Encode `bytes` as a lowercase hex string (no `0x` prefix).
fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b: &u8| format!("{b:02x}"))
        .collect::<Vec<String>>()
        .join("")
}

fn write_files(output: &GenerateOutput, out_dir: &Path) -> Result<(), PolyplugcError> {
    let summary: WriteSummary = write_output(output, out_dir)?;
    println!(
        "generated {} files ({} written, {} unchanged)",
        output.files.len(),
        summary.written,
        summary.unchanged
    );
    Ok(())
}
