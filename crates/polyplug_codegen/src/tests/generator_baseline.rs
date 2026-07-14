//! Byte-level compatibility baselines for the current generated binding surfaces.
//!
//! The checked-in manifest stores readable output classifications and SHA-256 digests
//! rather than opaque generated-source snapshots. Every digest is computed from the
//! bytes written by `write_output`, so Rust baselines include its rustfmt result.

#![allow(clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use sha2::Digest;
use sha2::Sha256;
use tempfile::tempdir;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::Lang;
use crate::OutputDestination;
use crate::OutputLayout;
use crate::OutputPartition;
use crate::Side;
use crate::ValidatedImport;
use crate::generate;
use crate::write_output;

const BASELINE: &str = include_str!("../../tests/fixtures/generator_baseline/baseline.tsv");

const LANGUAGES: [Lang; 6] = [
    Lang::Rust,
    Lang::Cpp,
    Lang::CSharp,
    Lang::Python,
    Lang::Lua,
    Lang::JsQuickJs,
];

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BaselineEntry {
    language: &'static str,
    side: &'static str,
    classification: &'static str,
    path: String,
    sha256: String,
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generator_baseline")
        .join(name)
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Host => "host",
        Side::Guest => "guest",
    }
}

fn classification(_lang: Lang, _side: Side, _path: &Path) -> &'static str {
    "canonical"
}

fn assert_unique_paths(output: &GenerateOutput, label: &str) {
    let mut paths: HashSet<&Path> = HashSet::with_capacity(output.files.len());
    for file in &output.files {
        assert!(
            paths.insert(file.path.as_path()),
            "{label} emitted duplicate output path `{}`",
            file.path.display()
        );
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_entries<F>(
    output: GenerateOutput,
    out_dir: &Path,
    lang: Lang,
    side: &'static str,
    classify: F,
    label: &str,
) -> Vec<BaselineEntry>
where
    F: Fn(&Path) -> &'static str,
{
    assert_unique_paths(&output, label);
    write_output(&output, out_dir).expect("write generated output");

    output
        .files
        .into_iter()
        .map(|file| {
            let path: String = file.path.to_string_lossy().into_owned();
            let bytes: Vec<u8> = fs::read(out_dir.join(&file.path))
                .unwrap_or_else(|error| panic!("read generated `{path}`: {error}"));
            BaselineEntry {
                language: lang.as_str(),
                side,
                classification: classify(&file.path),
                path,
                sha256: digest(&bytes),
            }
        })
        .collect()
}

fn parse_baseline() -> Vec<BaselineEntry> {
    BASELINE
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            assert_eq!(
                columns.len(),
                5,
                "baseline line must have five tab-separated columns: {line}"
            );
            BaselineEntry {
                language: columns[0],
                side: columns[1],
                classification: columns[2],
                path: columns[3].to_owned(),
                sha256: columns[4].to_owned(),
            }
        })
        .collect()
}

fn format_entries(entries: &[BaselineEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "{}\t{}\t{}\t{}\t{}",
                entry.language, entry.side, entry.classification, entry.path, entry.sha256
            )
        })
        .collect::<Vec<String>>()
        .join("\n")
}

#[test]
fn current_generated_disk_bytes_match_compatibility_baseline() {
    let temp: tempfile::TempDir = tempdir().expect("create temp directory");
    let api_path: PathBuf = fixture_path("api.toml");
    let bundle_path: PathBuf = fixture_path("bundle.toml");

    let mut actual: Vec<BaselineEntry> = Vec::new();
    for lang in LANGUAGES {
        for side in [Side::Host, Side::Guest] {
            let api_toml: PathBuf = match side {
                Side::Host => api_path.clone(),
                Side::Guest => bundle_path.clone(),
            };
            let output: GenerateOutput = generate(GenerateConfig {
                api_toml,
                lang,
                side,
                layout: OutputLayout::unified(),
            })
            .unwrap_or_else(|error| {
                panic!("generate {} {}: {error}", lang.as_str(), side_name(side))
            });
            let out_dir: PathBuf = temp.path().join(lang.as_str()).join(side_name(side));
            let classify = |path: &Path| classification(lang, side, path);
            actual.extend(write_entries(
                output,
                &out_dir,
                lang,
                side_name(side),
                classify,
                &format!("{} {}", lang.as_str(), side_name(side)),
            ));
        }
    }

    actual.sort();

    let mut expected: Vec<BaselineEntry> = parse_baseline();
    expected.sort();
    assert_eq!(
        actual,
        expected,
        "generated disk bytes or classification changed. Update the reviewed baseline only for intended compatibility changes:\n{}",
        format_entries(&actual)
    );
}

#[test]
fn rust_caller_layouts_keep_unified_baseline_and_split_canonical_types() {
    let temp = tempdir().expect("create output directory");
    let api_path = fixture_path("api.toml");
    let bundle_path = fixture_path("bundle.toml");

    let unified_guest = generate(GenerateConfig {
        api_toml: bundle_path.clone(),
        lang: Lang::Rust,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    })
    .expect("generate unified Rust guest");
    let unified_guest_entries = write_entries(
        unified_guest,
        &temp.path().join("unified-guest"),
        Lang::Rust,
        "guest",
        |_| "canonical",
        "unified Rust guest",
    );
    assert_eq!(
        unified_guest_entries
            .iter()
            .find(|entry| entry.path == "guest/peer_callers.rs")
            .expect("unified Rust peer callers")
            .sha256,
        "4ee3e749f180b50910041e994dba8b5084ae8ad8d2f4051ab77a5886eeead178",
        "unified Rust peer caller bytes are a compatibility contract"
    );

    let unified_host = generate(GenerateConfig {
        api_toml: api_path.clone(),
        lang: Lang::Rust,
        side: Side::Host,
        layout: OutputLayout::unified(),
    })
    .expect("generate unified Rust host");
    let unified_host_entries = write_entries(
        unified_host,
        &temp.path().join("unified-host"),
        Lang::Rust,
        "host",
        |_| "canonical",
        "unified Rust host",
    );
    assert_eq!(
        unified_host_entries
            .iter()
            .find(|entry| entry.path == "host/host_callers.rs")
            .expect("unified Rust host callers")
            .sha256,
        "b5f532fb23aa5ab5f5143cd78ffeb73b0c2f467ecc9927966e6bdec33bfb73cd",
        "unified Rust host caller bytes are a compatibility contract"
    );

    let guest_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::domain")
                .expect("valid domain import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::guest_contracts")
                .expect("valid guest-contract import"),
        },
    };
    let split_guest = generate(GenerateConfig {
        api_toml: bundle_path,
        lang: Lang::Rust,
        side: Side::Guest,
        layout: guest_layout,
    })
    .expect("generate split Rust guest");
    let peer_callers = split_guest
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/peer_callers.rs"))
        .expect("split Rust peer callers")
        .content
        .as_str();
    for required in [
        "use super::types::*;",
        "pub fn snapshot(&mut self) -> Result<common::domain::Payload, PeerCallError>",
        "self.arena.reset();",
        "Ok(common::domain::Payload",
    ] {
        assert!(
            peer_callers.contains(required),
            "split Rust peer caller must preserve canonical domain values and arena ownership: {required}"
        );
    }
    assert!(
        !peer_callers.contains("use common::domain::*;"),
        "peer callers must keep ABI types unambiguous while using canonical domain paths"
    );

    let split_host = generate(GenerateConfig {
        api_toml: api_path,
        lang: Lang::Rust,
        side: Side::Host,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::Rust, "common::domain")
                    .expect("valid domain import"),
            },
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate split Rust host");
    let host_callers = split_host
        .files
        .iter()
        .find(|file| file.path == Path::new("host/host_callers.rs"))
        .expect("split Rust host callers")
        .content
        .as_str();
    for required in [
        "payload: &common::domain::Payload",
        "bytes: Vec<u8>",
        "Buffer { ptr: (bytes).as_ptr().cast_mut(), len: (bytes).len(), cap: (bytes).len() }",
    ] {
        assert!(
            host_callers.contains(required),
            "split Rust host caller must lower canonical ownership safely: {required}"
        );
    }
}

#[test]
fn reset_templates_separate_destructive_lifecycle_from_dispatch_revalidation() {
    let api_path = fixture_path("api.toml");
    let expectations: &[(Lang, &str, &[&str])] = &[
        (
            Lang::Rust,
            "host/host_callers.rs",
            &[
                "let interface = if revision != self.cached_revision",
                "if interface.is_null() {",
                "self.interface = core::ptr::null();",
                "if interface == self.interface && !self.instance.data.is_null()",
                "self.cached_revision = self.live_revision();",
            ],
        ),
        (
            Lang::Cpp,
            "host/host_callers.hpp",
            &[
                "const GuestContractInterface* iface = revision == cached_revision_",
                "if (iface == nullptr) {",
                "interface_ = nullptr;",
                "if ((revision == cached_revision_ || iface == interface_) && instance_.data != nullptr)",
                "cached_revision_ = polyplug_load_revision(revision_host_);",
            ],
        ),
        (
            Lang::CSharp,
            "host/Callers.cs",
            &[
                "GuestContractInterface* iface = _interface;",
                "if (iface == null) {",
                "_interface = null;",
                "if (!_disposed && (revision == _cachedRevision || iface == _interface))",
                "_cachedRevision = LiveRevision();",
            ],
        ),
        (
            Lang::Python,
            "host/callers.py",
            &[
                "interface = self._interface",
                "if not interface:",
                "self._interface = None",
                "if revision == self._cached_revision or interface == self._interface:",
                "self._cached_revision = self._live_revision()",
            ],
        ),
        (
            Lang::Lua,
            "host/callers.lua",
            &[
                "local interface = self._host.resolve_guest_contract(self._host, self._handle)",
                "if interface == nil or interface ~= self._interface then",
                "local revision = self:live_revision()",
                "if interface == nil then",
                "if revision == self._cached_revision or interface == self._interface then",
            ],
        ),
    ];

    for (lang, path, expected_fragments) in expectations {
        let output = generate(GenerateConfig {
            api_toml: api_path.clone(),
            lang: *lang,
            side: Side::Host,
            layout: OutputLayout::unified(),
        })
        .unwrap_or_else(|error| panic!("generate {} host callers: {error}", lang.as_str()));
        let callers = output
            .files
            .into_iter()
            .find(|file| file.path == PathBuf::from(path))
            .unwrap_or_else(|| panic!("{} host callers missing `{path}`", lang.as_str()));

        for fragment in *expected_fragments {
            assert!(
                callers.content.contains(fragment),
                "{} reset lifecycle must emit `{fragment}`",
                lang.as_str()
            );
        }
    }
}

#[test]
fn caller_revalidation_invalidation_is_terminal_in_every_host_template() {
    let api_path = fixture_path("api.toml");
    let expectations: &[(Lang, &str, &[&str])] = &[
        (
            Lang::Rust,
            "host/host_callers.rs",
            &[
                "self.interface = core::ptr::null();",
                "self.instance = GuestContractInstance::null();",
                "if self.interface.is_null() || (self.live_revision() != self.cached_revision && !self.revalidate())",
            ],
        ),
        (
            Lang::Cpp,
            "host/host_callers.hpp",
            &[
                "interface_ = nullptr;",
                "instance_ = GuestContractInstance{};",
                "if (interface_ == nullptr || (polyplug_load_revision(revision_host_) != cached_revision_ && !revalidate()))",
            ],
        ),
        (
            Lang::CSharp,
            "host/Callers.cs",
            &[
                "_interface = null;",
                "_instance = default;",
                "_disposed = true;",
                "if (_interface == null || (LiveRevision() != _cachedRevision && !Revalidate()))",
            ],
        ),
        (
            Lang::Python,
            "host/callers.py",
            &[
                "self._interface = None",
                "self._instance = GuestContractInstance()",
                "if not self._interface or (self._live_revision() != self._cached_revision and not self._revalidate()):",
            ],
        ),
        (
            Lang::Lua,
            "host/callers.lua",
            &[
                "self._interface = nil",
                "self._instance = ffi.new(\"GuestContractInstance\")",
                "self._destroyed = true",
                "if self._interface == nil or self._destroyed then",
            ],
        ),
        (
            Lang::JsQuickJs,
            "host/callers.ts",
            &[
                "#destroyed: boolean;",
                "#invalidate(): void",
                "throw new Error('caller has already been destroyed');",
                "cachedView.destroyInstance(instance);",
                "if (this.#destroyed || this.#view === null || (this.#liveRevision() !== this.#cachedRevision && !this.#revalidate()))",
            ],
        ),
    ];

    for (lang, path, expected_fragments) in expectations {
        let output = generate(GenerateConfig {
            api_toml: api_path.clone(),
            lang: *lang,
            side: Side::Host,
            layout: OutputLayout::unified(),
        })
        .unwrap_or_else(|error| panic!("generate {} host callers: {error}", lang.as_str()));
        let callers = output
            .files
            .into_iter()
            .find(|file| file.path == PathBuf::from(path))
            .unwrap_or_else(|| panic!("{} host callers missing `{path}`", lang.as_str()));

        for fragment in *expected_fragments {
            assert!(
                callers.content.contains(fragment),
                "{} terminal invalidation must emit `{fragment}`",
                lang.as_str()
            );
        }
    }
}

#[test]
fn every_language_accepts_and_emits_the_canonical_split_layout() {
    let temporary_root = tempdir().expect("create split-layout root");
    let expectations: [(Lang, &str, &str); 6] = [
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

    for (language, domain_import, guest_contracts_import) in expectations {
        let layout = OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: temporary_root.path().join(language.as_str()).join("domain"),
                import: ValidatedImport::parse(language, domain_import).unwrap_or_else(|error| {
                    panic!("valid {} domain import: {error}", language.as_str())
                }),
            },
            guest_contracts: OutputDestination::Emit {
                root: temporary_root
                    .path()
                    .join(language.as_str())
                    .join("guest_contracts"),
                import: ValidatedImport::parse(language, guest_contracts_import).unwrap_or_else(
                    |error| panic!("valid {} guest-contract import: {error}", language.as_str()),
                ),
            },
        };
        let output = generate(GenerateConfig {
            api_toml: fixture_path("api.toml"),
            lang: language,
            side: Side::Guest,
            layout,
        })
        .unwrap_or_else(|error| {
            panic!(
                "{} must support the canonical split layout: {error}",
                language.as_str()
            )
        });

        for partition in [
            OutputPartition::Bindings,
            OutputPartition::DomainTypes,
            OutputPartition::GuestContracts,
        ] {
            assert!(
                output.files.iter().any(|file| file.partition == partition),
                "{} guest output must contain {}",
                language.as_str(),
                match partition {
                    OutputPartition::Bindings => "bindings",
                    OutputPartition::DomainTypes => "domain types",
                    OutputPartition::GuestContracts => "guest contracts",
                }
            );
        }
    }
}
