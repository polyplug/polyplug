//! Incremental-write and path-safety behaviour of the public writer.

#![allow(clippy::expect_used)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use tempfile::tempdir;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::GeneratedFile;
use crate::Lang;
use crate::OutputDestination;
use crate::OutputLayout;
use crate::OutputPartition;
use crate::PolyplugcError;
use crate::Side;
use crate::ValidatedImport;
use crate::WriteSummary;
use crate::generate;
use crate::generate::force_next_atomic_replace_failure;
use crate::generate::force_next_atomic_write_failure;
use crate::write_output;

const API_TOML: &str = "\
[[guest_contract]]
name = \"pipeline.Decoder\"
version = \"1.0.0\"

[[guest_contract.functions]]
name = \"decode\"
return = \"StringView\"
";

const BUNDLE_TOML: &str = "\
[bundle]
name = \"inc_write\"
version = \"1.0.0\"
api = \"api.toml\"
loader = \"lua\"
file = \"plugin.lua\"

[[plugin]]
name = \"decoder\"
implements = [\"pipeline.Decoder@1.0\"]
";

fn generate_lua_bundle(tmp_dir: &PathBuf) -> (GenerateOutput, PathBuf) {
    fs::create_dir_all(tmp_dir).expect("create tmp dir");
    let api_path: PathBuf = tmp_dir.join("api.toml");
    let bundle_path: PathBuf = tmp_dir.join("bundle.toml");
    fs::write(&api_path, API_TOML).expect("write api.toml");
    fs::write(&bundle_path, BUNDLE_TOML).expect("write bundle.toml");

    let out_dir: PathBuf = tmp_dir.join("out");
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_path,
        lang: Lang::Lua,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    };
    let output: GenerateOutput = generate(config).expect("generate guest");
    (output, out_dir)
}

#[test]
fn manifest_is_force_regenerate_and_bindings_are_not() {
    let tmp_dir: PathBuf =
        env::temp_dir().join(format!("polyplugc_inc_write_flags_{}", process::id()));
    let _ = fs::remove_dir_all(&tmp_dir);
    let (output, _out_dir): (GenerateOutput, PathBuf) = generate_lua_bundle(&tmp_dir);

    let manifest = output
        .files
        .iter()
        .find(|f| f.path.file_name().is_some_and(|n| n == "manifest.toml"))
        .expect("manifest.toml must be generated");
    assert!(
        manifest.force_regenerate,
        "manifest.toml must be force_regenerate (its ids must always be current)"
    );

    let lua_binding = output
        .files
        .iter()
        .find(|f| f.path.extension().is_some_and(|e| e == "lua"))
        .expect("a .lua binding must be generated");
    assert!(
        !lua_binding.force_regenerate,
        "language bindings must NOT be force_regenerate (so unchanged ones are cached)"
    );

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn rewrites_only_force_and_changed_files() {
    let tmp_dir: PathBuf =
        env::temp_dir().join(format!("polyplugc_inc_write_cache_{}", process::id()));
    let _ = fs::remove_dir_all(&tmp_dir);
    let (output, out_dir): (GenerateOutput, PathBuf) = generate_lua_bundle(&tmp_dir);

    let total: usize = output.files.len();
    let force_count: usize = output.files.iter().filter(|f| f.force_regenerate).count();
    assert!(
        force_count >= 1,
        "expected at least the manifest to be force_regenerate"
    );
    assert!(
        total > force_count,
        "expected at least one cacheable binding"
    );

    // First write: nothing on disk yet, so every file is written.
    let first: WriteSummary = write_output(&output, &out_dir).expect("first write");
    assert_eq!(first.written, total, "first write must emit every file");
    assert_eq!(first.unchanged, 0, "first write has nothing to skip");

    // Identical re-write: only the force-regenerate files are rewritten; every other
    // file is byte-identical on disk and skipped.
    let second: WriteSummary = write_output(&output, &out_dir).expect("second write");
    assert_eq!(
        second.written, force_count,
        "an identical re-run must rewrite only the force_regenerate files"
    );
    assert_eq!(
        second.unchanged,
        total - force_count,
        "every unchanged binding must be skipped"
    );

    // Drift one cached binding on disk: it must be rewritten back to canonical form.
    let victim = output
        .files
        .iter()
        .find(|f| !f.force_regenerate)
        .expect("a non-force binding exists");
    let victim_path: PathBuf = out_dir.join(&victim.path);
    fs::write(&victim_path, "-- stale, drifted content\n").expect("drift victim");

    let third: WriteSummary = write_output(&output, &out_dir).expect("third write");
    assert_eq!(
        third.written,
        force_count + 1,
        "the drifted binding plus the force files must be rewritten"
    );
    assert_eq!(third.unchanged, total - force_count - 1);

    let restored: String = fs::read_to_string(&victim_path).expect("read restored victim");
    assert_eq!(
        restored, victim.content,
        "the drifted binding must be restored to its generated content"
    );

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn unsafe_output_paths_are_rejected_before_writes() {
    let temp = tempdir().expect("create temporary directory");
    let out_dir = temp.path().join("generated");
    let unsafe_paths = [
        PathBuf::from("../outside.txt"),
        PathBuf::from("/tmp/polyplug_codegen_outside.txt"),
    ];

    for path in unsafe_paths {
        let output = GenerateOutput::from_files(
            Lang::Rust,
            OutputLayout::unified(),
            vec![
                GeneratedFile {
                    path: PathBuf::from("safe.txt"),
                    content: "safe".to_owned(),
                    force_regenerate: false,
                    partition: crate::OutputPartition::Bindings,
                    references: Vec::new(),
                },
                GeneratedFile {
                    path,
                    content: "unsafe".to_owned(),
                    force_regenerate: false,
                    partition: crate::OutputPartition::Bindings,
                    references: Vec::new(),
                },
            ],
        );
        let error =
            write_output(&output, &out_dir).expect_err("unsafe output path must be rejected");
        assert!(matches!(error, PolyplugcError::UnsafeOutputPath { .. }));
    }

    assert!(
        !out_dir.exists(),
        "writer must reject paths before creating output"
    );
}

#[test]
fn layout_writes_emit_partitions_to_distinct_roots_and_skips_import_only() {
    let temp = tempdir().expect("create temporary directory");
    let primary = temp.path().join("primary");
    let domain_root = temp.path().join("domain");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: ValidatedImport::parse(Lang::Rust, "shared::domain").expect("valid import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "shared::contracts").expect("valid import"),
        },
    };
    let output = GenerateOutput::from_files(
        Lang::Rust,
        layout,
        vec![
            GeneratedFile {
                path: PathBuf::from("types.rs"),
                content: "pub struct Private;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("types.rs"),
                content: "pub struct Domain;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("contracts.rs"),
                content: "pub trait Contract {}\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::GuestContracts,
                references: Vec::new(),
            },
        ],
    );

    let summary = write_output(&output, &primary).expect("write layout");
    assert_eq!(summary.written, 2);
    assert!(primary.join("types.rs").is_file());
    assert!(domain_root.join("types.rs").is_file());
    assert!(!primary.join("contracts.rs").exists());
}

#[test]
fn layout_preflight_rejects_colliding_targets_before_writing() {
    let temp = tempdir().expect("create temporary directory");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![
            GeneratedFile {
                path: PathBuf::from("same.rs"),
                content: "binding".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("same.rs"),
                content: "domain".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
        ],
    );

    let error = write_output(&output, temp.path()).expect_err("same target must be rejected");
    assert!(matches!(error, PolyplugcError::DuplicateOutputPath { .. }));
    assert!(!temp.path().join("same.rs").exists());
}

#[test]
fn layout_preflight_rejects_omitted_references_without_writing() {
    let temp = tempdir().expect("create temporary directory");
    let out = temp.path().join("out");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Omit,
        guest_contracts: OutputDestination::Omit,
    };
    let output = GenerateOutput::from_files(
        Lang::Rust,
        layout,
        vec![GeneratedFile {
            path: PathBuf::from("bindings.rs"),
            content: "pub fn adapter() {}\n".to_owned(),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: vec![OutputPartition::DomainTypes],
        }],
    );

    let error =
        write_output(&output, &out).expect_err("binding reference to omitted domain must fail");
    assert!(matches!(error, PolyplugcError::ValidationFailed { .. }));
    assert!(!out.exists());
}

#[test]
fn validated_import_rejects_path_traversal_and_invalid_rust_segments() {
    assert!(ValidatedImport::parse(Lang::Rust, "shared::domain").is_ok());
    assert!(ValidatedImport::parse(Lang::Rust, "shared/../domain").is_err());
    assert!(ValidatedImport::parse(Lang::Rust, "shared::not-valid").is_err());
    for invalid in [
        "1shared::domain",
        "_",
        "shared::self",
        "fn::domain",
        "crate::self",
    ] {
        assert!(
            ValidatedImport::parse(Lang::Rust, invalid).is_err(),
            "{invalid} must be rejected"
        );
    }
}

#[test]
fn layout_rejects_cross_language_imports_and_existing_file_ancestors() {
    let import = ValidatedImport::parse(Lang::Cpp, "shared/domain.hpp").expect("valid C++ import");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly { import },
        guest_contracts: OutputDestination::Omit,
    };
    assert!(layout.validate(Lang::Rust, &[]).is_err());

    let temp = tempdir().expect("create temporary directory");
    let root = temp.path().join("out");
    fs::write(&root, "not a directory").expect("write blocking file");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![GeneratedFile {
            path: PathBuf::from("nested/file.rs"),
            content: "pub struct Generated;\n".to_owned(),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        }],
    );
    assert!(matches!(
        write_output(&output, &root),
        Err(PolyplugcError::ValidationFailed { .. })
    ));
}

#[cfg(unix)]
#[test]
fn layout_preflight_rejects_alias_roots_without_writing() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("create temporary directory");
    let root = temp.path().join("root");
    let alias = temp.path().join("alias");
    fs::create_dir(&root).expect("create root");
    symlink(&root, &alias).expect("create root alias");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: alias,
            import: ValidatedImport::parse(Lang::Rust, "common::domain").expect("valid import"),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let output = GenerateOutput::from_files(
        Lang::Rust,
        layout,
        vec![
            GeneratedFile {
                path: PathBuf::from("same.rs"),
                content: "pub struct Binding;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("same.rs"),
                content: "pub struct Domain;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
        ],
    );
    assert!(matches!(
        write_output(&output, &root),
        Err(PolyplugcError::DuplicateOutputPath { .. })
    ));
    assert!(!root.join("same.rs").exists());
}

#[cfg(unix)]
#[test]
fn layout_preflight_rejects_alias_ancestor_targets_without_writing() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("create temporary directory");
    let root = temp.path().join("root");
    let alias = temp.path().join("alias");
    fs::create_dir(&root).expect("create root");
    symlink(&root, &alias).expect("create root alias");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: alias,
            import: ValidatedImport::parse(Lang::Rust, "common::domain").expect("valid import"),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let output = GenerateOutput::from_files(
        Lang::Rust,
        layout,
        vec![
            GeneratedFile {
                path: PathBuf::from("nested"),
                content: "binding".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("nested/generated.rs"),
                content: "pub struct Domain;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
        ],
    );
    assert!(matches!(
        write_output(&output, &root),
        Err(PolyplugcError::ValidationFailed { .. })
    ));
    assert!(
        !root.join("nested").exists(),
        "all alias-ancestor conflicts must fail before writing"
    );
}

#[cfg(unix)]
#[test]
fn layout_preflight_rejects_dangling_alias_root_collisions_before_writing() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("create temporary directory");
    let root = temp.path().join("root");
    let alias = temp.path().join("alias");
    let missing_target = temp.path().join("missing");
    fs::create_dir(&root).expect("create root");
    symlink(&missing_target, &alias).expect("create dangling root alias");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: alias,
            import: ValidatedImport::parse(Lang::Rust, "common::domain").expect("valid import"),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let output = GenerateOutput::from_files(
        Lang::Rust,
        layout,
        vec![
            GeneratedFile {
                path: PathBuf::from("same.rs"),
                content: "pub struct Binding;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("same.rs"),
                content: "pub struct Domain;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
        ],
    );

    assert!(matches!(
        write_output(&output, &root),
        Err(PolyplugcError::ValidationFailed { .. })
    ));
    assert!(!root.join("same.rs").exists());
    assert!(!missing_target.exists());
}

#[cfg(unix)]
#[test]
fn layout_preflight_rejects_dangling_alias_ancestor_targets_before_writing() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("create temporary directory");
    let root = temp.path().join("root");
    let alias = temp.path().join("alias");
    let missing_target = temp.path().join("missing");
    fs::create_dir(&root).expect("create root");
    symlink(&missing_target, &alias).expect("create dangling root alias");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: alias,
            import: ValidatedImport::parse(Lang::Rust, "common::domain").expect("valid import"),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let output = GenerateOutput::from_files(
        Lang::Rust,
        layout,
        vec![
            GeneratedFile {
                path: PathBuf::from("nested"),
                content: "binding".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("nested/generated.rs"),
                content: "pub struct Domain;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
        ],
    );

    assert!(matches!(
        write_output(&output, &root),
        Err(PolyplugcError::ValidationFailed { .. })
    ));
    assert!(!root.join("nested").exists());
    assert!(!missing_target.exists());
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[test]
fn layout_preflight_rejects_windows_and_macos_case_only_targets() {
    let temp = tempdir().expect("create temporary directory");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![
            GeneratedFile {
                path: PathBuf::from("Generated.rs"),
                content: "pub struct First;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: PathBuf::from("generated.rs"),
                content: "pub struct Second;\n".to_owned(),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            },
        ],
    );
    assert!(matches!(
        write_output(&output, temp.path()),
        Err(PolyplugcError::DuplicateOutputPath { .. })
    ));
    assert!(
        !temp.path().join("Generated.rs").exists(),
        "case-only conflicts must fail before writing"
    );
}

#[test]
fn force_regenerate_replaces_existing_content() {
    let temp = tempdir().expect("create temporary directory");
    let path = temp.path().join("generated.txt");
    fs::write(&path, "stale").expect("write stale generated file");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![GeneratedFile {
            path: PathBuf::from("generated.txt"),
            content: "canonical".to_owned(),
            force_regenerate: true,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        }],
    );

    let first = write_output(&output, temp.path()).expect("replace stale generated file");
    assert_eq!(
        first,
        WriteSummary {
            written: 1,
            unchanged: 0
        }
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read replaced generated file"),
        "canonical"
    );
    let second = write_output(&output, temp.path()).expect("force rewrite generated file");
    assert_eq!(
        second,
        WriteSummary {
            written: 1,
            unchanged: 0
        }
    );
}

#[test]
fn atomic_write_failure_removes_temporary_file() {
    let temp = tempdir().expect("create temporary directory");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![GeneratedFile {
            path: PathBuf::from("generated.rs"),
            content: "pub struct Generated;\n".to_owned(),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        }],
    );
    force_next_atomic_write_failure();
    assert!(matches!(
        write_output(&output, temp.path()),
        Err(PolyplugcError::WriteFailed { .. })
    ));
    assert!(!temp.path().join("generated.rs").exists());
    assert!(
        fs::read_dir(temp.path())
            .expect("read temporary directory")
            .next()
            .is_none(),
        "failed writes must clean temporary files"
    );
}

#[test]
fn atomic_replace_failure_removes_temporary_file() {
    let temp = tempdir().expect("create temporary directory");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![GeneratedFile {
            path: PathBuf::from("generated.rs"),
            content: "pub struct Generated;\n".to_owned(),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        }],
    );
    force_next_atomic_replace_failure();
    assert!(matches!(
        write_output(&output, temp.path()),
        Err(PolyplugcError::WriteFailed { .. })
    ));
    assert!(!temp.path().join("generated.rs").exists());
    let generated_entries: Vec<_> = fs::read_dir(temp.path())
        .expect("read temporary directory")
        .collect();
    assert!(
        generated_entries.is_empty(),
        "failed writes must clean temporary files"
    );
}
