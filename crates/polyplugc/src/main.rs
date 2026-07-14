use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use clap::Subcommand;

use ed25519_dalek::SigningKey;
use ed25519_dalek::VerifyingKey;
use polyplug_codegen::GenerateConfig;
use polyplug_codegen::GenerateOutput;
use polyplug_codegen::InternalCSharpGenerateConfig;
use polyplug_codegen::InternalCppGenerateConfig;
use polyplug_codegen::InternalJavaScriptGenerateConfig;
use polyplug_codegen::InternalLuaGenerateConfig;
use polyplug_codegen::InternalPythonGenerateConfig;
use polyplug_codegen::InternalRustGenerateConfig;
use polyplug_codegen::Lang;
use polyplug_codegen::OutputDestination;
use polyplug_codegen::OutputLayout;
use polyplug_codegen::PolyplugcError;
use polyplug_codegen::Side;
use polyplug_codegen::ValidatedImport;
use polyplug_codegen::WriteSummary;
use polyplug_codegen::generate;
use polyplug_codegen::generate_internal_cpp;
use polyplug_codegen::generate_internal_csharp;
use polyplug_codegen::generate_internal_javascript;
use polyplug_codegen::generate_internal_lua;
use polyplug_codegen::generate_internal_python;
use polyplug_codegen::generate_internal_rust;
use polyplug_codegen::parse_lang;
use polyplug_codegen::parser;
use polyplug_codegen::write_output;
use polyplug_signing::{
    SigError, VerifiedBundle, generate_keypair, load_signing_key, save_signing_key,
    save_verifying_key, sign_bundle, verify_bundle,
};

mod validate;

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

#[derive(Debug)]
struct PartitionOutputArgs {
    out: Option<PathBuf>,
    import: Option<String>,
    omit: bool,
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

        /// Generate matching guest provider and host caller bindings for one internal
        /// plugin bundle.
        #[arg(long, requires = "bundle")]
        internal: bool,

        /// Target language: rust, cpp, csharp, python, lua, js-quickjs.
        #[arg(long, short = 'l')]
        lang: String,

        /// Output directory for generated files.
        #[arg(long, short = 'o', required = true)]
        out: PathBuf,

        /// Separate root for application-owned domain types. Requires
        /// `--domain-types-import` so bindings can reference the emitted module.
        #[arg(long, requires = "domain_types_import")]
        domain_types_out: Option<PathBuf>,

        /// Language-specific import used by generated bindings for external domain
        /// types.
        #[arg(long)]
        domain_types_import: Option<String>,

        /// Do not emit application-owned domain types.
        #[arg(
            long,
            conflicts_with_all = ["domain_types_out", "domain_types_import"]
        )]
        domain_types_omit: bool,

        /// Separate root for guest contract declarations. Requires
        /// `--guest-contracts-import` so bindings can reference the emitted module.
        #[arg(long, requires = "guest_contracts_import")]
        guest_contracts_out: Option<PathBuf>,

        /// Language-specific import used by generated bindings for external guest
        /// contracts.
        #[arg(long)]
        guest_contracts_import: Option<String>,

        /// Do not emit guest contract declarations.
        #[arg(
            long,
            conflicts_with_all = ["guest_contracts_out", "guest_contracts_import"]
        )]
        guest_contracts_omit: bool,
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
            internal,
            lang,
            out,
            domain_types_out,
            domain_types_import,
            domain_types_omit,
            guest_contracts_out,
            guest_contracts_import,
            guest_contracts_omit,
        } => {
            let language: Lang = parse_lang(&lang)?;
            let layout: OutputLayout = output_layout(
                language,
                PartitionOutputArgs {
                    out: domain_types_out,
                    import: domain_types_import,
                    omit: domain_types_omit,
                },
                PartitionOutputArgs {
                    out: guest_contracts_out,
                    import: guest_contracts_import,
                    omit: guest_contracts_omit,
                },
            )?;
            let output: GenerateOutput = if internal {
                let bundle_toml: PathBuf =
                    bundle.ok_or_else(|| PolyplugcError::ValidationFailed {
                        message: "--internal requires --bundle".to_owned(),
                    })?;
                match language {
                    Lang::Rust => generate_internal_rust(InternalRustGenerateConfig {
                        bundle_toml,
                        layout: layout.clone(),
                    })?,
                    Lang::Cpp => generate_internal_cpp(InternalCppGenerateConfig {
                        bundle_toml,
                        out_dir: out.clone(),
                        layout: layout.clone(),
                    })?,
                    Lang::CSharp => generate_internal_csharp(InternalCSharpGenerateConfig {
                        bundle_toml,
                        out_dir: out.clone(),
                        layout: layout.clone(),
                    })?,
                    Lang::Python => generate_internal_python(InternalPythonGenerateConfig {
                        bundle_toml,
                        out_dir: out.clone(),
                        layout: layout.clone(),
                    })?,
                    Lang::Lua => generate_internal_lua(InternalLuaGenerateConfig {
                        bundle_toml,
                        out_dir: out.clone(),
                        layout: layout.clone(),
                    })?,
                    Lang::JsQuickJs => {
                        generate_internal_javascript(InternalJavaScriptGenerateConfig {
                            bundle_toml,
                            out_dir: out.clone(),
                            layout: layout.clone(),
                        })?
                    }
                }
            } else {
                let (manifest, side): (PathBuf, Side) = if let Some(api_path) = api {
                    (api_path, Side::Host)
                } else if let Some(bundle_path) = bundle {
                    (bundle_path, Side::Guest)
                } else {
                    return Err(PolyplugcError::ValidationFailed {
                        message: "Must specify --api or --bundle".to_owned(),
                    });
                };
                generate(GenerateConfig {
                    api_toml: manifest,
                    lang: language,
                    side,
                    layout: layout.clone(),
                })?
            };
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

fn output_layout(
    lang: Lang,
    domain_types: PartitionOutputArgs,
    guest_contracts: PartitionOutputArgs,
) -> Result<OutputLayout, PolyplugcError> {
    Ok(OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: output_destination(lang, "--domain-types-out", domain_types)?,
        guest_contracts: output_destination(lang, "--guest-contracts-out", guest_contracts)?,
    })
}

fn output_destination(
    lang: Lang,
    out_flag: &str,
    args: PartitionOutputArgs,
) -> Result<OutputDestination, PolyplugcError> {
    if args.omit {
        if args.out.is_some() || args.import.is_some() {
            return Err(PolyplugcError::ValidationFailed {
                message: format!("{out_flag} cannot be combined with its omit flag"),
            });
        }
        return Ok(OutputDestination::Omit);
    }

    match (args.out, args.import) {
        (None, None) => Ok(OutputDestination::Inline),
        (Some(_), None) => Err(PolyplugcError::ValidationFailed {
            message: format!("{out_flag} requires its matching import flag"),
        }),
        (Some(root), Some(import)) => Ok(OutputDestination::Emit {
            root,
            import: ValidatedImport::parse(lang, import)?,
        }),
        (None, Some(import)) => Ok(OutputDestination::ImportOnly {
            import: ValidatedImport::parse(lang, import)?,
        }),
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Lang;
    use super::OutputDestination;
    use super::PartitionOutputArgs;
    use super::output_layout;

    #[test]
    fn output_layout_routes_emit_import_only_and_omit_for_every_language() {
        let cases: [(Lang, &str, &str); 6] = [
            (Lang::Rust, "shared::domain", "shared::guest_contracts"),
            (Lang::Cpp, "guest/domain.hpp", "guest/guest_contracts.hpp"),
            (Lang::CSharp, "Shared.Domain", "Shared.GuestContracts"),
            (Lang::Python, "shared.domain", "shared.guest_contracts"),
            (Lang::Lua, "shared.domain", "shared.guest_contracts"),
            (
                Lang::JsQuickJs,
                "@test/javascript-domain",
                "@test/javascript-contracts",
            ),
        ];

        for (language, domain_import, guest_contracts_import) in cases {
            let emitted = output_layout(
                language,
                PartitionOutputArgs {
                    out: Some(PathBuf::from("domain")),
                    import: Some(domain_import.to_owned()),
                    omit: false,
                },
                PartitionOutputArgs {
                    out: Some(PathBuf::from("guest_contracts")),
                    import: Some(guest_contracts_import.to_owned()),
                    omit: false,
                },
            )
            .unwrap_or_else(|error| panic!("valid {} emitted layout: {error}", language.as_str()));
            assert!(matches!(
                emitted.domain_types,
                OutputDestination::Emit { .. }
            ));
            assert!(matches!(
                emitted.guest_contracts,
                OutputDestination::Emit { .. }
            ));

            let imported = output_layout(
                language,
                PartitionOutputArgs {
                    out: None,
                    import: Some(domain_import.to_owned()),
                    omit: false,
                },
                PartitionOutputArgs {
                    out: None,
                    import: Some(guest_contracts_import.to_owned()),
                    omit: false,
                },
            )
            .unwrap_or_else(|error| {
                panic!("valid {} import-only layout: {error}", language.as_str())
            });
            assert!(matches!(
                imported.domain_types,
                OutputDestination::ImportOnly { .. }
            ));
            assert!(matches!(
                imported.guest_contracts,
                OutputDestination::ImportOnly { .. }
            ));

            let omitted = output_layout(
                language,
                PartitionOutputArgs {
                    out: None,
                    import: None,
                    omit: true,
                },
                PartitionOutputArgs {
                    out: None,
                    import: None,
                    omit: true,
                },
            )
            .unwrap_or_else(|error| panic!("valid {} omitted layout: {error}", language.as_str()));
            assert!(matches!(omitted.domain_types, OutputDestination::Omit));
            assert!(matches!(omitted.guest_contracts, OutputDestination::Omit));

            assert!(
                output_layout(
                    language,
                    PartitionOutputArgs {
                        out: Some(PathBuf::from("domain")),
                        import: None,
                        omit: false,
                    },
                    PartitionOutputArgs {
                        out: None,
                        import: None,
                        omit: false,
                    },
                )
                .is_err(),
                "{} output roots must require imports",
                language.as_str()
            );
            assert!(
                output_layout(
                    language,
                    PartitionOutputArgs {
                        out: None,
                        import: Some("../outside".to_owned()),
                        omit: false,
                    },
                    PartitionOutputArgs {
                        out: None,
                        import: None,
                        omit: false,
                    },
                )
                .is_err(),
                "{} must reject invalid imports",
                language.as_str()
            );
        }
    }
}
