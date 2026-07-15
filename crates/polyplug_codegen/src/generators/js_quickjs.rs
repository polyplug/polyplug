//! js_quickjs generator — produces QuickJS-compatible TypeScript/JavaScript guest code.
//!
//! THIS FILE IS PART OF polyplugc.
//! Generates code using lo/hi u32 split for all u64/pointer values.

use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::PathBuf;

use super::CodeGenerator;
use super::GeneratedFile;
use super::GeneratedFiles;
use super::canonical_pascal_case;
use super::collect_peer_contracts;
use super::internal_generation_fingerprint;
use super::peer_min_version;

use super::attributes::render_attributes;
use super::docs::write_jsdoc;
use crate::Lang;
use crate::OutputDestination;
use crate::OutputLayout;
use crate::OutputPartition;
use crate::PolyplugcError;
use crate::Side;
use crate::ir::AbiBuiltin;
use crate::ir::CustomizableNode;
use crate::ir::EnumDef;
use crate::ir::EnumVariant;
use crate::ir::LanguageRules;
use crate::ir::PrimitiveType;
use crate::ir::ReprType;
use crate::ir::ResolvedBundle;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedField;
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedHostContract;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;
use crate::ir::array_element_name;
use langprint::backends::js_backend::{
    JsBackend, JsEnum, JsEnumMember, JsFunction, JsFunctionRenderOptions, JsParameter,
};
use langprint::renderers::{EnumRenderer, FunctionRenderer};
use langprint::{ImportEntry, ImportSet, TargetLanguage};
use polyplug_utils::guest_contract_id;
use std::io;

fn js_attribute_lines(node: CustomizableNode, rules: &LanguageRules, indent: &str) -> String {
    render_attributes(Lang::JsQuickJs, node, rules)
        .into_iter()
        .map(|attribute| format!("{indent}{attribute}\n"))
        .collect()
}

fn emit_js_attributes(
    out: &mut String,
    node: CustomizableNode,
    rules: &LanguageRules,
    indent: &str,
) {
    out.push_str(&js_attribute_lines(node, rules, indent));
}

fn emit_js_function_attributes(out: &mut String, function: &ResolvedFunction, indent: &str) {
    emit_js_attributes(out, CustomizableNode::Function, &function.langs, indent);
    for param in &function.params {
        emit_js_attributes(out, CustomizableNode::Param, &param.langs, indent);
    }
    emit_js_attributes(
        out,
        CustomizableNode::Return,
        &function.return_langs,
        indent,
    );
}

/// A JS/TS named import `import {{ {name} }} from '{source}'`.
fn js_named(name: &str, source: &str) -> ImportEntry {
    ImportEntry::JsNamed {
        name: name.to_string(),
        source: source.to_string(),
    }
}

/// A JS named re-export `export {{ {name} }} from '{source}'`.
fn js_reexport(name: &str, source: &str) -> ImportEntry {
    ImportEntry::JsReexport {
        name: name.to_string(),
        source: source.to_string(),
    }
}

/// Render grouped JS/TS import & re-export statements through langprint's
/// [`ImportSet`] so the syntax + dedup + grouping live in one place rather than
/// in hand-written `push_str("import …")` sequences. [`ImportSet`] merges
/// same-source entries onto one `{ a, b }` line and emits a fixed kind order
/// (default, named, type-named, `type * as`, re-export). Each inner slice is one
/// blank-line-separated group; empty groups are skipped, so a caller can pass a
/// conditional group unconditionally. The result ends in a single newline (empty
/// when every group is empty); callers append a blank line before the body.
fn js_import_block(groups: &[&[ImportEntry]]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for group in groups {
        let mut set: ImportSet = ImportSet::new(TargetLanguage::Js);
        for entry in *group {
            set.add(entry.clone());
        }
        let rendered: String = set.render();
        if !rendered.is_empty() {
            blocks.push(rendered);
        }
    }
    blocks.join("\n")
}
use std::string::FromUtf8Error;

fn has_domain_types(ir: &ValidatedIr) -> bool {
    !ir.enums.is_empty() || !ir.types.is_empty()
}

fn domain_type_references(ir: &ValidatedIr) -> Vec<OutputPartition> {
    has_domain_types(ir)
        .then_some(OutputPartition::DomainTypes)
        .into_iter()
        .collect()
}

/// Generator for js-quickjs plugin bundles.
///
/// Produces TypeScript files using lo/hi u32 pairs for 64-bit values
/// (QuickJS uses f64 internally, so bigint is not available).
pub struct JsQuickjsGenerator;

impl JsQuickjsGenerator {
    /// Generate the opt-in JavaScript internal-plugin profile without changing
    /// the ordinary host or external guest output.
    pub(crate) fn generate_internal_bundle(
        &self,
        ir: &ValidatedIr,
        _bundle_name: &str,
        layout: &OutputLayout,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        let split: bool = layout != &OutputLayout::unified();
        let fingerprint = internal_generation_fingerprint(ir);
        if !split {
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/types.ts"),
                content: generate_types_ts(ir)?,
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/callers.ts"),
                content: with_ts_internal_fingerprint(
                    generate_internal_callers_ts(ir)?,
                    fingerprint,
                ),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/types.ts"),
                content: with_ts_internal_fingerprint(generate_types_ts(ir)?, fingerprint),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/contracts.ts"),
                content: with_ts_internal_fingerprint(
                    generate_internal_contracts_ts(ir)?.replace("\"./types\"", "\"./types.ts\""),
                    fingerprint,
                ),
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
            files.files.push(GeneratedFile {
                path: PathBuf::from("internal.ts"),
                content: generate_internal_profile_ts(ir)?,
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
            return Ok(());
        }

        let domain_module: String =
            js_partition_module(layout, OutputPartition::DomainTypes, "../domain/types.ts");
        let guest_contracts_omitted: bool = matches!(
            layout.destination(OutputPartition::GuestContracts),
            OutputDestination::Omit
        );
        let domain_types_present: bool = has_domain_types(ir);
        let contracts_module: String =
            js_partition_module(layout, OutputPartition::GuestContracts, "./contracts.ts");
        if domain_types_present {
            files.files.push(GeneratedFile {
                path: PathBuf::from("domain/types.ts"),
                content: with_ts_internal_fingerprint(generate_domain_types_ts(ir)?, fingerprint),
                force_regenerate: false,
                partition: OutputPartition::DomainTypes,
                references: Vec::new(),
            });
        }
        if !guest_contracts_omitted {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/contracts.ts"),
                content: with_ts_internal_fingerprint(
                    generate_guest_contract_declarations_ts(ir, &domain_module)?,
                    fingerprint,
                ),
                force_regenerate: false,
                partition: OutputPartition::GuestContracts,
                references: domain_type_references(ir),
            });
        }
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/callers.ts"),
            content: with_ts_internal_fingerprint(
                with_js_domain_type_imports(generate_internal_callers_ts(ir)?, &domain_module, ir)?,
                fingerprint,
            ),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: domain_type_references(ir),
        });
        let mut guest_binding_references: Vec<OutputPartition> = domain_type_references(ir);
        if !guest_contracts_omitted {
            guest_binding_references.push(OutputPartition::GuestContracts);
        }
        let guest_binding_content: String = generate_internal_contracts_ts_with_type_source(
            ir,
            &domain_module,
            guest_contracts_omitted,
            true,
        )?;
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/bindings.ts"),
            content: with_ts_internal_fingerprint(
                if guest_contracts_omitted {
                    guest_binding_content
                } else {
                    with_js_guest_contract_type_imports(
                        guest_binding_content,
                        &contracts_module,
                        ir,
                    )?
                },
                fingerprint,
            ),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: guest_binding_references,
        });
        let mut semantic_sources = Vec::new();
        if domain_types_present {
            semantic_sources.push(domain_module.as_str());
        }
        if !guest_contracts_omitted {
            semantic_sources.push(contracts_module.as_str());
        }
        files.files.push(GeneratedFile {
            path: PathBuf::from("internal.ts"),
            content: generate_internal_profile_ts_with_modules(
                ir,
                &domain_module,
                "./host/callers.ts",
                "./guest/bindings.ts",
                &semantic_sources,
            )?,
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: domain_type_references(ir),
        });
        Ok(())
    }
}

fn with_ts_internal_fingerprint(mut content: String, fingerprint: u64) -> String {
    content.push_str(&format!(
        "\nexport const _polyplugInternalGenerationFingerprint = 0x{fingerprint:016X}n;\n"
    ));
    content
}

impl CodeGenerator for JsQuickjsGenerator {
    fn generate_host(
        &self,
        ir: &ValidatedIr,
        layout: &OutputLayout,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        let split: bool = layout != &OutputLayout::unified();
        let domain_module: String =
            js_partition_module(layout, OutputPartition::DomainTypes, "./types");
        let domain_types_present: bool = has_domain_types(ir);
        if !split || domain_types_present {
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/types.ts"),
                content: if split {
                    generate_domain_types_ts(ir)?
                } else {
                    generate_types_ts(ir)?
                },
                force_regenerate: false,
                partition: if split {
                    OutputPartition::DomainTypes
                } else {
                    OutputPartition::Bindings
                },
                references: Vec::new(),
            });
        }
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/callers.ts"),
            content: if split {
                with_js_domain_type_imports(generate_callers_ts(ir)?, &domain_module, ir)?
            } else {
                generate_callers_ts(ir)?
            },
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: if split {
                domain_type_references(ir)
            } else {
                Vec::new()
            },
        });
        if !ir.host_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/contracts.ts"),
                content: if split {
                    with_js_domain_type_imports(generate_host_contracts_ts(ir), &domain_module, ir)?
                } else {
                    generate_host_contracts_ts(ir)
                },
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: if split {
                    domain_type_references(ir)
                } else {
                    Vec::new()
                },
            });
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/interface_factories.ts"),
                content: if split {
                    with_js_domain_type_imports(
                        generate_js_host_interface_factories_ts(ir)?,
                        &domain_module,
                        ir,
                    )?
                } else {
                    generate_js_host_interface_factories_ts(ir)?
                },
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: if split {
                    domain_type_references(ir)
                } else {
                    Vec::new()
                },
            });
        }
        Ok(())
    }

    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        layout: &OutputLayout,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        let split: bool = layout != &OutputLayout::unified();
        let domain_module: String =
            js_partition_module(layout, OutputPartition::DomainTypes, "./types");
        let guest_contracts_omitted: bool = matches!(
            layout.destination(OutputPartition::GuestContracts),
            OutputDestination::Omit
        );
        let contracts_module: String =
            js_partition_module(layout, OutputPartition::GuestContracts, "./contracts");
        let domain_types_present: bool = has_domain_types(ir);
        if !split || domain_types_present {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/types.ts"),
                content: if split {
                    generate_domain_types_ts(ir)?
                } else {
                    generate_types_ts(ir)?
                },
                force_regenerate: false,
                partition: if split {
                    OutputPartition::DomainTypes
                } else {
                    OutputPartition::Bindings
                },
                references: Vec::new(),
            });
        }
        if split {
            if !guest_contracts_omitted {
                files.files.push(GeneratedFile {
                    path: PathBuf::from("guest/contracts.ts"),
                    content: generate_guest_contract_declarations_ts(ir, &domain_module)?,
                    force_regenerate: false,
                    partition: OutputPartition::GuestContracts,
                    references: domain_type_references(ir),
                });
            }
            let mut guest_binding_references: Vec<OutputPartition> = domain_type_references(ir);
            if !guest_contracts_omitted {
                guest_binding_references.push(OutputPartition::GuestContracts);
            }
            let guest_binding_content: String = generate_contracts_ts_with_type_source(
                ir,
                &domain_module,
                guest_contracts_omitted,
                true,
            )?;
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/bindings.ts"),
                content: if guest_contracts_omitted {
                    guest_binding_content
                } else {
                    with_js_guest_contract_type_imports(
                        guest_binding_content,
                        &contracts_module,
                        ir,
                    )?
                },
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: guest_binding_references,
            });
        } else {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/contracts.ts"),
                content: generate_contracts_ts(ir)?,
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
        }
        let binding_module: &str = if split { "./bindings" } else { "./contracts" };
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/interface.ts"),
            content: generate_interface_ts_with_bindings(ir, binding_module),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/init.ts"),
            content: generate_init_ts_with_bindings(ir, binding_module),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/index.ts"),
            content: generate_index_ts_with_bindings(ir, binding_module),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        });
        if ir.bundle.is_some() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("manifest.toml"),
                content: generate_manifest_toml(ir),
                force_regenerate: true,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
        }
        if !ir.host_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/host_contracts.ts"),
                content: if split {
                    with_js_domain_type_imports(
                        generate_guest_host_contracts_ts(ir)?,
                        &domain_module,
                        ir,
                    )?
                } else {
                    generate_guest_host_contracts_ts(ir)?
                },
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: if split {
                    domain_type_references(ir)
                } else {
                    Vec::new()
                },
            });
        }
        let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
        if !peer_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/peer_callers.ts"),
                content: if split {
                    with_js_domain_type_imports(
                        generate_guest_peer_callers_ts(ir, &peer_contracts)?,
                        &domain_module,
                        ir,
                    )?
                } else {
                    generate_guest_peer_callers_ts(ir, &peer_contracts)?
                },
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: if split {
                    domain_type_references(ir)
                } else {
                    Vec::new()
                },
            });
        }
        files.files.push(GeneratedFile {
            path: PathBuf::from("README.md"),
            content: generate_readme_quickjs(ir),
            force_regenerate: false,
            partition: OutputPartition::Bindings,
            references: Vec::new(),
        });
        Ok(())
    }

    fn apply_output_layout(
        &self,
        _ir: &ValidatedIr,
        _side: Side,
        _layout: &OutputLayout,
        _files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        Ok(())
    }
}

/// Map a resolved type reference to its TypeScript representation (QuickJS lo/hi style).
fn ts_type_ref(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => ts_primitive(p).to_owned(),
        ResolvedTypeRef::AbiType(b) => ts_abi_builtin(b).to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

fn ts_primitive(p: &PrimitiveType) -> &'static str {
    match p {
        PrimitiveType::U8
        | PrimitiveType::U16
        | PrimitiveType::U32
        | PrimitiveType::I8
        | PrimitiveType::I16
        | PrimitiveType::I32
        | PrimitiveType::F32
        | PrimitiveType::F64 => "number",
        PrimitiveType::Bool => "boolean",
        PrimitiveType::U64 | PrimitiveType::I64 => "{ lo: number; hi: number }",
    }
}

fn ts_abi_builtin(b: &AbiBuiltin) -> &'static str {
    match b {
        AbiBuiltin::StringView => "{ ptr_lo: number; ptr_hi: number; len: number }",
        AbiBuiltin::Buffer => "{ ptr_lo: number; ptr_hi: number; len: number; cap: number }",
        AbiBuiltin::Ptr => "{ lo: number; hi: number }",
        AbiBuiltin::Void => "void",
    }
}

fn substitute_variant_refs_js(declared_variants: &[EnumVariant], expr: &str) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let len: usize = chars.len();
    let mut result: String = String::new();
    let mut i: usize = 0;
    while i < len {
        let c: char = chars[i];
        if c.is_alphabetic() || c == '_' {
            let start: usize = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            let found: Option<&EnumVariant> = declared_variants.iter().find(|v| v.name == ident);
            if let Some(ref_variant) = found {
                result.push('(');
                result.push_str(&ref_variant.value);
                result.push(')');
            } else {
                result.push_str(&ident);
            }
        } else {
            result.push(c);
            i += 1;
        }
    }
    result
}

fn with_js_enum_variant_attributes(rendered: String, e: &EnumDef) -> String {
    if e.variants.iter().all(|variant| {
        js_attribute_lines(CustomizableNode::EnumVariant, &variant.langs, "").is_empty()
    }) {
        return rendered;
    }

    let mut output = String::new();
    for line in rendered.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(variant) = e.variants.iter().find(|variant| {
            trimmed
                .strip_prefix(&variant.name)
                .is_some_and(|suffix| suffix.starts_with(" =") || suffix.starts_with(": "))
        }) {
            let indent = &line[..line.len() - trimmed.len()];
            output.push_str(&js_attribute_lines(
                CustomizableNode::EnumVariant,
                &variant.langs,
                indent,
            ));
        }
        output.push_str(line);
    }
    output
}

fn generate_js_quickjs_enum(out: &mut String, e: &EnumDef) -> Result<(), PolyplugcError> {
    let doc: String = if e.bitflag {
        format!("@bitflag Enum {}", e.name)
    } else {
        format!("Enum {}", e.name)
    };
    let mut docs: Vec<String> = super::docs::lines(e.docs.as_deref())
        .into_iter()
        .map(str::to_owned)
        .collect();
    for variant in &e.variants {
        if let Some(variant_docs) = variant.docs.as_deref() {
            docs.push(format!(
                "{}: {}",
                variant.name,
                variant_docs.replace('\n', " ")
            ));
        }
    }
    emit_js_attributes(out, CustomizableNode::Enum, &e.langs, "");
    if !docs.is_empty() {
        write_jsdoc(out, "", Some(&docs.join("\n")), &[], None);
    }
    let js_enum: JsEnum = JsEnum {
        name: e.name.clone(),
        members: e
            .variants
            .iter()
            .map(|variant| JsEnumMember {
                name: variant.name.clone(),
                value: substitute_variant_refs_js(&e.variants, &variant.value),
            })
            .collect(),
        doc: Some(doc),
        export: true,
    };
    // QuickJS output is 4-space indented, not the JS-idiomatic 2.
    let backend: JsBackend = JsBackend {
        indent_size: 4,
        ..JsBackend::default()
    };
    let mut indent_level: i32 = 0;
    let rendered: String = with_js_enum_variant_attributes(
        backend
            .render_enum(
                &js_enum,
                None::<&str>,
                None::<&str>,
                None,
                &mut indent_level,
            )
            .map_err(|source: io::Error| PolyplugcError::WriteFailed {
                path: "types.ts".to_owned(),
                source,
            })?,
        e,
    );
    out.push_str(&rendered);
    // Golden separates each enum from the following declaration with a blank line;
    // the renderer ends the companion `type` line with a single newline.
    out.push('\n');
    Ok(())
}
fn generate_types_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");
    for e in &ir.enums {
        generate_js_quickjs_enum(&mut out, e)?;
    }
    for type_def in &ir.types {
        render_resolved_type(&mut out, type_def);
    }
    for contract in &ir.contracts {
        render_contract_types(&mut out, contract);
    }
    Ok(out)
}

fn generate_domain_types_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");
    for e in &ir.enums {
        generate_js_quickjs_enum(&mut out, e)?;
    }
    for type_def in &ir.types {
        render_resolved_type(&mut out, type_def);
    }
    Ok(out)
}

fn generate_guest_contract_declarations_ts(
    ir: &ValidatedIr,
    domain_module: &str,
) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs guest contract declarations\n\n",
    );
    let imports: String = js_import_block(&[&js_domain_type_entries(ir, domain_module)]);
    out.push_str(&imports);
    if !imports.is_empty() {
        out.push('\n');
    }
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");
    for contract in &ir.contracts {
        render_guest_contract_types(&mut out, contract);
    }
    Ok(out)
}

fn js_partition_module(
    layout: &OutputLayout,
    partition: OutputPartition,
    inline_module: &str,
) -> String {
    layout
        .destination(partition)
        .import()
        .map(|import| import.as_str().to_owned())
        .unwrap_or_else(|| inline_module.to_owned())
}

fn js_domain_type_entries(ir: &ValidatedIr, source: &str) -> Vec<ImportEntry> {
    ir.enums
        .iter()
        .map(|enum_def| enum_def.name.clone())
        .chain(ir.types.iter().map(|type_def| type_def.name.clone()))
        .collect::<BTreeSet<String>>()
        .into_iter()
        .map(|name| ImportEntry::JsTypeNamed {
            name,
            source: source.to_owned(),
        })
        .collect()
}

fn with_js_domain_type_imports(
    content: String,
    source: &str,
    ir: &ValidatedIr,
) -> Result<String, PolyplugcError> {
    insert_js_type_imports(content, js_domain_type_entries(ir, source))
}

fn js_contract_function_type_name(
    contract: &ResolvedContract,
    function: &ResolvedFunction,
) -> String {
    format!("{}_{}", contract.name.replace('.', "_"), function.name)
}

fn js_guest_contract_type_entries(ir: &ValidatedIr, source: &str) -> Vec<ImportEntry> {
    ir.contracts
        .iter()
        .flat_map(|contract| {
            contract
                .functions
                .iter()
                .map(move |function| js_contract_function_type_name(contract, function))
        })
        .collect::<BTreeSet<String>>()
        .into_iter()
        .map(|name| ImportEntry::JsTypeNamed {
            name,
            source: source.to_owned(),
        })
        .collect()
}

fn with_js_guest_contract_type_imports(
    content: String,
    source: &str,
    ir: &ValidatedIr,
) -> Result<String, PolyplugcError> {
    insert_js_type_imports(content, js_guest_contract_type_entries(ir, source))
}

fn insert_js_type_imports(
    mut content: String,
    entries: Vec<ImportEntry>,
) -> Result<String, PolyplugcError> {
    let imports: String = js_import_block(&[&entries]);
    if imports.is_empty() {
        return Ok(content);
    }
    let header_end: usize =
        content
            .find("\n\n")
            .ok_or_else(|| PolyplugcError::ValidationFailed {
                message: "generated TypeScript file is missing its header separator".to_owned(),
            })?;
    content.insert_str(header_end + 2, &format!("{imports}\n"));
    Ok(content)
}

fn render_resolved_type(out: &mut String, type_def: &ResolvedType) {
    emit_js_attributes(out, CustomizableNode::Type, &type_def.langs, "");
    write_jsdoc(out, "", type_def.docs.as_deref(), &[], None);
    out.push_str(&format!("export interface {} {{\n", type_def.name));
    for field in &type_def.fields {
        render_resolved_field(out, field);
    }
    out.push_str("}\n\n");
}

fn render_resolved_field(out: &mut String, field: &ResolvedField) {
    let ts_t: String = ts_type_ref(&field.ty);
    emit_js_attributes(out, CustomizableNode::Field, &field.langs, "    ");
    write_jsdoc(out, "    ", field.docs.as_deref(), &[], None);
    out.push_str(&format!("    readonly {}: {};\n", field.name, ts_t));
}

fn render_contract_types(out: &mut String, contract: &ResolvedContract) {
    render_contract_types_with_surface(out, contract, false);
}

fn render_guest_contract_types(out: &mut String, contract: &ResolvedContract) {
    render_contract_types_with_surface(out, contract, true);
}

fn render_contract_types_with_surface(
    out: &mut String,
    contract: &ResolvedContract,
    guest_provider_surface: bool,
) {
    emit_js_attributes(out, CustomizableNode::GuestContract, &contract.langs, "");
    write_jsdoc(out, "", contract.docs.as_deref(), &[], None);
    for func in &contract.functions {
        let params: String = func
            .params
            .iter()
            .map(|p: &ResolvedParam| format!("{}: {}", p.name, ts_type_ref(&p.ty)))
            .collect::<Vec<String>>()
            .join(", ");
        let ret_type: String = js_contract_return_type(func, guest_provider_surface);
        let documented_params: Vec<(&str, Option<&str>)> = func
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.docs.as_deref()))
            .collect();
        emit_js_function_attributes(out, func, "");
        write_jsdoc(
            out,
            "",
            func.docs.as_deref(),
            &documented_params,
            func.return_docs.as_deref(),
        );
        out.push_str(&format!(
            "export type {} = ({}) => {};\n",
            js_contract_function_type_name(contract, func),
            params,
            ret_type
        ));
    }
}

fn js_contract_return_type(function: &ResolvedFunction, guest_provider_surface: bool) -> String {
    match &function.returns {
        None => "void".to_owned(),
        Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) if guest_provider_surface => {
            "string".to_owned()
        }
        Some(ty) => ts_type_ref(ty),
    }
}

fn generate_contracts_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    generate_contracts_ts_with_type_source(ir, "./types", false, false)
}

fn generate_contracts_ts_with_type_source(
    ir: &ValidatedIr,
    type_source: &str,
    local_contract_types: bool,
    use_contract_type_aliases: bool,
) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    // Collect the user-defined type names referenced by the rendered plugin
    // interfaces (setXFactory signatures use them) — an empty type import leaves
    // `AddArgs`-style references dangling under `deno check`.
    let mut type_imports: BTreeSet<String> = BTreeSet::new();
    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    for func in &contract.functions {
                        for param in &func.params {
                            if let ResolvedTypeRef::UserDefined(name) = &param.ty {
                                type_imports.insert(name.clone());
                            }
                        }
                        if let Some(ResolvedTypeRef::UserDefined(name)) = &func.returns {
                            type_imports.insert(name.clone());
                        }
                    }
                }
            }
        }
    }
    let type_entries: Vec<ImportEntry> = type_imports
        .iter()
        .map(|n: &String| ImportEntry::JsTypeNamed {
            name: n.clone(),
            source: type_source.to_owned(),
        })
        .collect();
    let block: String = js_import_block(&[&type_entries]);
    out.push_str(&block);
    if !block.is_empty() {
        out.push('\n');
    }
    if local_contract_types {
        for contract in &ir.contracts {
            render_guest_contract_types(&mut out, contract);
        }
        if !ir.contracts.is_empty() {
            out.push('\n');
        }
    }
    out.push_str("/** Dispatch mechanism type — determines how function calls are routed. */\n");
    out.push_str("const DispatchType = Object.freeze({\n");
    out.push_str("    Native: 0,\n");
    out.push_str("    VirtualMachine: 1,\n");
    out.push_str("} as const);\n\n");
    emit_ts_utf8_encoder_helper(&mut out)?;

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
                    let set_factory_name: String = js_factory_setter_name(&plugin.name);
                    render_plugin_interface_quickjs(
                        &mut out,
                        JsPluginInterfaceConfig {
                            plugin_name: &plugin.name,
                            contract,
                            ir,
                            interface_var: &plugin_var,
                            set_factory_name: &set_factory_name,
                            export_wrappers: false,
                            use_contract_type_aliases,
                        },
                    )?;
                }
            }
        }
    }

    Ok(out)
}

fn js_factory_setter_name(plugin_name: &str) -> String {
    format!(
        "set{}Factory",
        plugin_name
            .replace(['.', '-'], "_")
            .split('_')
            .map(|segment| {
                let mut chars = segment.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<String>()
    )
}

fn js_internal_provider_symbol(plugin_name: &str, contract_name: &str) -> String {
    format!(
        "{}_{}",
        plugin_name.to_uppercase().replace(['.', '-'], "_"),
        contract_name.to_uppercase().replace(['.', '-'], "_")
    )
}

fn js_internal_provider_field(plugin_name: &str, contract_name: &str) -> String {
    format!(
        "{}_{}",
        plugin_name.to_lowercase().replace(['.', '-'], "_"),
        contract_name.to_lowercase().replace(['.', '-'], "_")
    )
}

fn js_internal_provider_entries(
    ir: &ValidatedIr,
) -> Result<Vec<(&ResolvedPlugin, &ResolvedContract)>, PolyplugcError> {
    let bundle: &ResolvedBundle =
        ir.bundle
            .as_ref()
            .ok_or_else(|| PolyplugcError::ValidationFailed {
                message: "JavaScript internal generation requires a bundle manifest".to_owned(),
            })?;
    let mut entries: Vec<(&ResolvedPlugin, &ResolvedContract)> = Vec::new();
    for plugin in &bundle.plugins {
        for implemented in &plugin.implements {
            let contract: &ResolvedContract = ir
                .contracts
                .iter()
                .find(|candidate| {
                    implemented
                        == &format!(
                            "{}@{}.{}",
                            candidate.name, candidate.version.major, candidate.version.minor
                        )
                })
                .ok_or_else(|| PolyplugcError::ValidationFailed {
                    message: format!(
                        "JavaScript internal generation could not resolve `{implemented}`"
                    ),
                })?;
            entries.push((plugin, contract));
        }
    }
    Ok(entries)
}

fn js_internal_type_entries(
    entries: &[(&ResolvedPlugin, &ResolvedContract)],
    source: &str,
) -> Vec<ImportEntry> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for (_, contract) in entries {
        for function in &contract.functions {
            for parameter in &function.params {
                if let ResolvedTypeRef::UserDefined(name) = &parameter.ty {
                    names.insert(name.clone());
                }
            }
            if let Some(ResolvedTypeRef::UserDefined(name)) = &function.returns {
                names.insert(name.clone());
            }
        }
    }
    names
        .into_iter()
        .map(|name| ImportEntry::JsTypeNamed {
            name,
            source: source.to_owned(),
        })
        .collect()
}

fn generate_internal_contracts_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    generate_internal_contracts_ts_with_type_source(ir, "./types", false, false)
}

fn generate_internal_contracts_ts_with_type_source(
    ir: &ValidatedIr,
    type_source: &str,
    local_contract_types: bool,
    use_contract_type_aliases: bool,
) -> Result<String, PolyplugcError> {
    let entries: Vec<(&ResolvedPlugin, &ResolvedContract)> = js_internal_provider_entries(ir)?;
    let mut out: String = String::from(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs internal provider bindings\n\n",
    );
    let imports: String = js_import_block(&[&js_internal_type_entries(&entries, type_source)]);
    out.push_str(&imports);
    if !imports.is_empty() {
        out.push('\n');
    }
    if local_contract_types {
        for contract in &ir.contracts {
            render_guest_contract_types(&mut out, contract);
        }
        if !ir.contracts.is_empty() {
            out.push('\n');
        }
    }
    out.push_str("const DispatchType = Object.freeze({ VirtualMachine: 1 } as const);\n\n");
    emit_ts_utf8_encoder_helper(&mut out)?;
    for (plugin, contract) in entries {
        let symbol: String = js_internal_provider_symbol(&plugin.name, &contract.name);
        let setter: String = format!("set{}Factory", symbol);
        render_plugin_interface_quickjs(
            &mut out,
            JsPluginInterfaceConfig {
                plugin_name: &plugin.name,
                contract,
                ir,
                interface_var: &symbol,
                set_factory_name: &setter,
                export_wrappers: true,
                use_contract_type_aliases,
            },
        )?;
    }
    Ok(out)
}

fn js_internal_author_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        _ => ts_type_ref(ty),
    }
}

fn generate_internal_profile_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    generate_internal_profile_ts_with_modules(
        ir,
        "./guest/types.ts",
        "./host/callers.ts",
        "./guest/contracts.ts",
        &["./guest/types.ts", "./guest/contracts.ts"],
    )
}

fn generate_internal_profile_ts_with_modules(
    ir: &ValidatedIr,
    type_source: &str,
    callers_source: &str,
    wrappers_source: &str,
    semantic_sources: &[&str],
) -> Result<String, PolyplugcError> {
    let entries: Vec<(&ResolvedPlugin, &ResolvedContract)> = js_internal_provider_entries(ir)?;
    let mut out: String = String::from(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Explicit JavaScript internal-plugin registration profile.\n\n",
    );
    out.push_str(
        r#"import { buildInternalPluginBundle, buildInternalPluginGuestContract, createInternalPluginGuestBridge } from "@polyplug/host";
import { bridgeLibrary } from "@polyplug/loaders/js";
"#,
    );
    let imports: String = js_import_block(&[&js_internal_type_entries(&entries, type_source)]);
    out.push_str(&imports);
    let caller_classes: BTreeSet<String> = entries
        .iter()
        .map(|(_, contract)| contract_to_class_name(&contract.name))
        .collect();
    for class in caller_classes {
        out.push_str(&format!(
            "import {{ {class}Contract }} from {callers_source:?};\n"
        ));
    }
    let wrapper_imports: Vec<String> = entries
        .iter()
        .flat_map(|(plugin, contract)| {
            let symbol: String = js_internal_provider_symbol(&plugin.name, &contract.name);
            (0..contract.functions.len())
                .map(|index| format!("{}_fn{index}_abi_wrapper", symbol.to_lowercase()))
                .collect::<Vec<String>>()
        })
        .collect();
    if !wrapper_imports.is_empty() {
        out.push_str(&format!(
            "import {{ {} }} from {wrappers_source:?};\n",
            wrapper_imports.join(", ")
        ));
    }
    let fingerprint = internal_generation_fingerprint(ir);
    let mut checks = Vec::new();
    for (name, source) in [
        ("callersFingerprint", callers_source),
        ("bindingsFingerprint", wrappers_source),
    ] {
        out.push_str(&format!(
            "import {{ _polyplugInternalGenerationFingerprint as {name} }} from {source:?};\n"
        ));
        checks.push(format!("{name} !== _polyplugInternalGenerationFingerprint"));
    }
    for (index, source) in semantic_sources.iter().enumerate() {
        let name = format!("semanticFingerprint{index}");
        out.push_str(&format!(
            "import {{ _polyplugInternalGenerationFingerprint as {name} }} from {source:?};\n"
        ));
        checks.push(format!("{name} !== _polyplugInternalGenerationFingerprint"));
    }
    out.push_str(&format!(
        "export const _polyplugInternalGenerationFingerprint = 0x{fingerprint:016X}n;\n\
         if ({}) throw new Error(\"generated internal partitions are incompatible\");\n",
        if checks.is_empty() {
            "false".to_owned()
        } else {
            checks.join(" || ")
        }
    ));
    out.push_str("\nexport interface InternalRuntime {\n    registerInternalPluginWithHandles(bundle: { dispose(): void }, handleCount: number): { bundleId: bigint; handles: unknown[] };\n    unloadBundle(bundleId: bigint): void;\n}\n\n");
    for (plugin, contract) in &entries {
        let field: String = js_internal_provider_field(&plugin.name, &contract.name);
        out.push_str(&format!("export interface {field}Implementation {{\n"));
        for function in &contract.functions {
            let parameters: String = function
                .params
                .iter()
                .map(|parameter| {
                    format!(
                        "{}: {}",
                        parameter.name,
                        js_internal_author_type(&parameter.ty)
                    )
                })
                .collect::<Vec<String>>()
                .join(", ");
            let returns: String = function
                .returns
                .as_ref()
                .map_or_else(|| "void".to_owned(), js_internal_author_type);
            out.push_str(&format!(
                "    {}({parameters}): {returns};\n",
                function.name
            ));
        }
        out.push_str("}\n\n");
    }
    out.push_str("type ProviderFactories = {\n");
    for (plugin, contract) in &entries {
        let field: String = js_internal_provider_field(&plugin.name, &contract.name);
        out.push_str(&format!("    {field}: () => {field}Implementation;\n"));
    }
    out.push_str("};\n\nexport class InternalProviders {\n    #factories: ProviderFactories | null;\n    constructor(factories: ProviderFactories) { this.#factories = factories; }\n    consume(): ProviderFactories {\n        if (this.#factories === null) throw new Error(\"internal provider input has already been consumed; create fresh providers\");\n        const factories = this.#factories;\n        this.#factories = null;\n        return factories;\n    }\n}\n\n");
    for (plugin, contract) in &entries {
        let field: String = js_internal_provider_field(&plugin.name, &contract.name);
        out.push_str(&format!(
            "function _adapt_{field}(factory: () => {field}Implementation): Record<string, (...args: any[]) => any> {{\n    const implementation = factory();\n    return {{\n"
        ));
        for (index, function) in contract.functions.iter().enumerate() {
            let arguments: String = function
                .params
                .iter()
                .enumerate()
                .map(|(parameter_index, _)| format!("arg{parameter_index}"))
                .collect::<Vec<String>>()
                .join(", ");
            let parameters: String = function
                .params
                .iter()
                .enumerate()
                .map(|(parameter_index, _)| format!("arg{parameter_index}: any"))
                .collect::<Vec<String>>()
                .join(", ");
            out.push_str(&format!(
                "        fn{index}: ({parameters}) => implementation.{}({arguments}),\n",
                function.name
            ));
        }
        out.push_str("    };\n}\n\n");
    }
    out.push_str("export interface Registration {\n    bundleId: bigint;\n");
    for (plugin, contract) in &entries {
        let field: String = js_internal_provider_field(&plugin.name, &contract.name);
        out.push_str(&format!(
            "    {field}: {}Contract;\n",
            contract_to_class_name(&contract.name)
        ));
    }
    out.push_str("}\n\nexport function register(runtime: InternalRuntime, providers: InternalProviders): Registration {\n    const factories = providers.consume();\n    const nativeBridge = bridgeLibrary();\n    const memory = createInternalPluginGuestBridge(nativeBridge);\n    const bundle = buildInternalPluginBundle({\n");
    out.push_str(&format!(
        "        manifest: {:?},\n        contracts: [\n",
        generate_manifest_toml(ir)
    ));
    for (plugin, contract) in &entries {
        let field: String = js_internal_provider_field(&plugin.name, &contract.name);
        let symbol: String = js_internal_provider_symbol(&plugin.name, &contract.name);
        out.push_str(&format!(
            "            {{ provider: {:?}, contractName: {:?}, version: {{ major: {}, minor: {}, patch: {} }},\n                adapter: buildInternalPluginGuestContract({{ contractId: 0x{:016X}n, version: {{ major: {}, minor: {}, patch: {} }}, implementation: host => {{ memory.setHost(host); return _adapt_{field}(factories.{field}); }}, methods: [\n",
            plugin.name,
            contract.name,
            contract.version.major,
            contract.version.minor,
            contract.version.patch,
            contract.contract_id,
            contract.version.major,
            contract.version.minor,
            contract.version.patch
        ));
        for index in 0..contract.functions.len() {
            out.push_str(&format!(
                "                    (implementation, args, out, arena) => {symbol_lower}_fn{index}_abi_wrapper(implementation, memory.addressOf(args), memory.addressOf(out), memory.addressOf(arena), memory),\n",
                symbol_lower = symbol.to_lowercase()
            ));
        }
        out.push_str("                ] }, nativeBridge) },\n");
    }
    out.push_str(&format!(
        "        ],\n    }});\n    let published: {{ bundleId: bigint; handles: unknown[] }};\n    try {{ published = runtime.registerInternalPluginWithHandles(bundle, {}); }} catch (error) {{ bundle.dispose(); throw error; }}\n    const bundleId = published.bundleId;\n",
        entries.len()
    ));
    for (plugin, contract) in &entries {
        let field: String = js_internal_provider_field(&plugin.name, &contract.name);
        let class: String = contract_to_class_name(&contract.name);
        out.push_str(&format!(
            "    const {field} = {class}Contract.createFromHandle(runtime as never, published.handles[{index}] as never);\n",
            index = entries
                .iter()
                .position(|(candidate_plugin, candidate_contract)| {
                    candidate_plugin.name == plugin.name && candidate_contract.name == contract.name
                })
                .unwrap_or(0)
        ));
        out.push_str(&format!(
            "    if ({field} === null) {{ runtime.unloadBundle(bundleId); throw new Error(\"registered contract caller was not discoverable\"); }}\n"
        ));
    }
    out.push_str("    return { bundleId");
    for (plugin, contract) in &entries {
        out.push_str(&format!(
            ", {}",
            js_internal_provider_field(&plugin.name, &contract.name)
        ));
    }
    out.push_str(" };\n}\n");
    Ok(out)
}

struct JsPluginInterfaceConfig<'a> {
    plugin_name: &'a str,
    contract: &'a ResolvedContract,
    ir: &'a ValidatedIr,
    interface_var: &'a str,
    set_factory_name: &'a str,
    export_wrappers: bool,
    use_contract_type_aliases: bool,
}

fn render_plugin_interface_quickjs(
    out: &mut String,
    config: JsPluginInterfaceConfig<'_>,
) -> Result<(), PolyplugcError> {
    let JsPluginInterfaceConfig {
        plugin_name,
        contract,
        ir,
        interface_var,
        set_factory_name,
        export_wrappers,
        use_contract_type_aliases,
    } = config;
    let plugin_var: String = interface_var.to_owned();
    let contract_name_full: String = format!("{}@{}", contract.name, contract.version.major);
    let contract_id: u64 = guest_contract_id(&contract.name, contract.version.major);
    let contract_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let function_count: usize = contract.functions.len();
    let version_major: u32 = contract.version.major;
    let version_minor: u32 = contract.version.minor;
    let version_patch: u32 = contract.version.patch;
    let impl_members: Vec<String> = contract
        .functions
        .iter()
        .enumerate()
        .map(|(idx, function)| {
            if use_contract_type_aliases {
                format!(
                    "fn{idx}: {}",
                    js_contract_function_type_name(contract, function)
                )
            } else {
                let params: String = function
                    .params
                    .iter()
                    .map(|param| format!("{}: {}", param.name, ts_type_ref(&param.ty)))
                    .collect::<Vec<String>>()
                    .join(", ");
                let ret: String = js_contract_return_type(function, true);
                format!("fn{idx}: ({params}) => {ret}")
            }
        })
        .collect();
    let impl_shape: String = format!("{{ {} }}", impl_members.join("; "));
    let implementation_surface: String = if use_contract_type_aliases {
        let implementation_type: String = format!("{plugin_var}Provider");
        out.push_str(&format!("\ntype {implementation_type} = {impl_shape};\n"));
        implementation_type
    } else {
        impl_shape.clone()
    };
    let wrapper_impl_type: &str = if use_contract_type_aliases {
        &implementation_surface
    } else {
        "any"
    };
    let interface_factory_type: &str = if use_contract_type_aliases {
        &implementation_surface
    } else {
        "any"
    };

    out.push_str(&format!(
        "// Plugin: {plugin_name} ({contract_name_full})\n"
    ));
    write_jsdoc(out, "", contract.docs.as_deref(), &[], None);
    for func in &contract.functions {
        let params: String = func
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, ts_type_ref(&param.ty)))
            .collect::<Vec<String>>()
            .join(", ");
        let ret_type: String = js_contract_return_type(func, use_contract_type_aliases);
        let documented_params: Vec<(&str, Option<&str>)> = func
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.docs.as_deref()))
            .collect();
        write_jsdoc(
            out,
            "",
            func.docs.as_deref(),
            &documented_params,
            func.return_docs.as_deref(),
        );
        out.push_str(&format!(
            "//   {fn_name}({params}): {ret_type}\n",
            fn_name = func.name
        ));
    }

    out.push_str(&format!("\nexport const {plugin_var}_INTERFACE = {{\n"));
    out.push_str(&format!("    contractLo: 0x{:08X},\n", contract_lo));
    out.push_str(&format!("    contractHi: 0x{:08X},\n", contract_hi));
    out.push_str("    dispatchType: DispatchType.VirtualMachine,\n");
    out.push_str(&format!("    fnCount: {function_count},\n"));
    out.push_str(&format!(
        "    functions: [] as ((impl: {wrapper_impl_type}, args_ptr: number, out_ptr: number, arena_ptr: number, bridge: any) => number)[],\n",
    ));
    out.push_str(&format!(
        "    factory: null as null | ((bridge: any, hostLo: number, hostHi: number) => {interface_factory_type}),\n",
    ));
    out.push_str(&format!("    contractName: \"{contract_name_full}\",\n"));
    out.push_str(&format!("    version: 0x{:08X},\n", version_major << 16));
    out.push_str("};\n");

    out.push_str(&format!("\nexport const {plugin_var}_DESCRIPTOR = {{\n"));
    out.push_str(&format!("    name: \"{plugin_name}\",\n"));
    out.push_str(&format!("    contractName: \"{contract_name_full}\",\n"));
    out.push_str(&format!("    version: {{ major: {version_major}, minor: {version_minor}, patch: {version_patch} }}\n"));
    out.push_str("};\n");

    let set_factory_name: String = set_factory_name.to_owned();
    let mut abi_wrappers: Vec<String> = Vec::new();
    for (idx, func) in contract.functions.iter().enumerate() {
        let wrapper_name: String = format!("{}_fn{}_abi_wrapper", plugin_var.to_lowercase(), idx);
        let has_params: bool = !func.params.is_empty();
        let has_return: bool = func.returns.is_some();
        let mut body: String = String::new();
        body.push_str("    // SAFETY: args_ptr and out_ptr are valid addresses passed as f64\n");
        body.push_str("    // by the loader. readU32/writeU32 accept f64 and convert to usize.\n");
        body.push_str(
            "    // `impl` is the per-instance impl object the loader resolved for this\n",
        );
        body.push_str(
            "    // call (built by the factory); the loader passes it as the first argument.\n",
        );
        body.push_str("    const polyplug = bridge;\n");
        body.push_str("    if (!polyplug) return 1;\n");
        body.push_str("    if (!impl) return 1;\n");

        if has_params {
            body.push_str("    if (!args_ptr) return 8;\n");
        }
        if has_return {
            body.push_str("    if (!out_ptr) return 8;\n");
        }

        let mut arg_names: Vec<String> = Vec::new();
        if func.params.len() == 1 {
            let param: &ResolvedParam = &func.params[0];
            let expr: String = js_read_expr(&param.ty, "args_ptr", 0, ir)?;
            body.push_str(&format!("    var arg_{} = {};\n", param.name, expr));
            arg_names.push(format!("arg_{}", param.name));
        } else if func.params.len() >= 2 {
            let mut offset: usize = 0;
            for param in &func.params {
                let align: usize = js_c_align(&param.ty, ir)?;
                offset = align_up(offset, align);
                let expr: String = js_read_expr(&param.ty, "args_ptr", offset, ir)?;
                body.push_str(&format!("    var arg_{} = {};\n", param.name, expr));
                offset += js_c_size(&param.ty, ir)?;
                arg_names.push(format!("arg_{}", param.name));
            }
        }

        if has_return {
            body.push_str(&format!(
                "    var result = impl.fn{idx}({});\n",
                arg_names.join(", ")
            ));
        } else {
            body.push_str(&format!("    impl.fn{idx}({});\n", arg_names.join(", ")));
        }

        if let Some(ret_ty) = &func.returns {
            emit_js_guest_return_write(&mut body, ret_ty, ir)?;
        }
        body.push_str("    return 0;");
        out.push('\n');
        out.push_str(&render_js_defn_fn(
            &wrapper_name,
            js_params(&[
                ("impl", wrapper_impl_type),
                ("args_ptr", "number"),
                ("out_ptr", "number"),
                ("arena_ptr", "number"),
                ("bridge", "any"),
            ]),
            Some("number".to_owned()),
            body,
            export_wrappers,
        )?);
        abi_wrappers.push(wrapper_name);
    }

    let functions_list: String = abi_wrappers
        .iter()
        .map(|wrapper| wrapper.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    out.push('\n');
    out.push_str(&render_js_set_factory(
        &set_factory_name,
        &plugin_var,
        &implementation_surface,
        &functions_list,
    )?);

    Ok(())
}

/// Render an exported `setXFactory` function via langprint's JS TypeScript mode.
/// polyplugc supplies the type strings (factory arrow type, interface variable);
/// langprint emits the typed signature + body. Byte-identical to the former
/// hand-written form (QuickJS output has no formatter).
fn render_js_set_factory(
    set_factory_name: &str,
    plugin_var: &str,
    implementation_surface: &str,
    functions_list: &str,
) -> Result<String, PolyplugcError> {
    let factory_type: String =
        format!("(bridge: any, hostLo: number, hostHi: number) => {implementation_surface}");
    let function: JsFunction = JsFunction {
        name: set_factory_name.to_owned(),
        parameters: vec![JsParameter {
            name: "factory".to_owned(),
            default: None,
            type_doc: Some(factory_type),
        }],
        return_type: Some("void".to_owned()),
        doc: None,
        is_static: false,
        body: Some(vec![
            format!("{plugin_var}_INTERFACE.factory = factory;"),
            format!("{plugin_var}_INTERFACE.functions = [{functions_list}];"),
        ]),
    };
    // polyplugc's QuickJS output indents 4, not the JS-idiomatic 2.
    let backend: JsBackend = JsBackend {
        indent_size: 4,
        ..JsBackend::default()
    };
    let options: JsFunctionRenderOptions = JsFunctionRenderOptions {
        render_jsdoc: false,
        typescript: true,
        verbatim_body: false,
    };
    let mut indent_level: i32 = 0;
    backend
        .render_function(
            &function,
            Some("export "),
            None::<&str>,
            Some(&options),
            &mut indent_level,
        )
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: "guest/contracts.ts".to_owned(),
            source,
        })
}

/// Render a QuickJS guest function DEFINITION via langprint's TypeScript mode with
/// a verbatim body. langprint owns the FORM (`[export ]function name(params): ret`);
/// polyplugc owns the body, passed as one verbatim String (exact whitespace and
/// nested indentation baked in, no trailing newline) — QuickJS output has no formatter.
fn render_js_defn_fn(
    name: &str,
    parameters: Vec<JsParameter>,
    return_type: Option<String>,
    body: String,
    export: bool,
) -> Result<String, PolyplugcError> {
    let function: JsFunction = JsFunction {
        name: name.to_owned(),
        parameters,
        return_type,
        doc: None,
        is_static: false,
        body: Some(vec![body]),
    };
    // polyplugc's QuickJS output indents 4, not the JS-idiomatic 2.
    let backend: JsBackend = JsBackend {
        indent_size: 4,
        ..JsBackend::default()
    };
    let options: JsFunctionRenderOptions = JsFunctionRenderOptions {
        render_jsdoc: false,
        typescript: true,
        verbatim_body: true,
    };
    let before: Option<&str> = if export { Some("export ") } else { None };
    let mut indent_level: i32 = 0;
    backend
        .render_function(
            &function,
            before,
            None::<&str>,
            Some(&options),
            &mut indent_level,
        )
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: "guest/contracts.ts".to_owned(),
            source,
        })
}

/// Render a class-member method DEFINITION (indent level 1, inside a hand-emitted
/// `class { … }`) via langprint's TypeScript method mode with a verbatim body.
/// langprint owns the FORM (`[static] name(params): ret`); polyplugc owns the body
/// and the single-line JSDoc (baked into `jsdoc`, emitted before the method).
fn render_js_method(
    name: &str,
    is_static: bool,
    parameters: Vec<JsParameter>,
    return_type: Option<String>,
    jsdoc: &str,
    body: String,
) -> Result<String, PolyplugcError> {
    let function: JsFunction = JsFunction {
        name: name.to_owned(),
        parameters,
        return_type,
        doc: None,
        is_static,
        body: Some(vec![body]),
    };
    // polyplugc's QuickJS output indents 4, not the JS-idiomatic 2.
    let backend: JsBackend = JsBackend {
        indent_size: 4,
        ..JsBackend::default()
    };
    let options: JsFunctionRenderOptions = JsFunctionRenderOptions {
        render_jsdoc: false,
        typescript: true,
        verbatim_body: true,
    };
    let before: String = jsdoc
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<String>>()
        .join("\n")
        + "\n";
    let mut indent_level: i32 = 1;
    let mut buf: Vec<u8> = Vec::new();
    backend
        .render_method_to(
            &function,
            Some(before.as_str()),
            None::<&str>,
            Some(&options),
            &mut indent_level,
            &mut buf,
        )
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: "guest/host_contracts.ts".to_owned(),
            source,
        })?;
    String::from_utf8(buf).map_err(|source: FromUtf8Error| PolyplugcError::WriteFailed {
        path: "guest/host_contracts.ts".to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })
}

/// Build a `JsParameter` list from `(name, ts_type)` pairs (TypeScript mode reads
/// each type from `type_doc`).
fn js_params(pairs: &[(&str, &str)]) -> Vec<JsParameter> {
    pairs
        .iter()
        .map(|(name, ty): &(&str, &str)| JsParameter {
            name: (*name).to_owned(),
            default: None,
            type_doc: Some((*ty).to_owned()),
        })
        .collect::<Vec<JsParameter>>()
}

fn generate_interface_ts(ir: &ValidatedIr) -> String {
    let bundle: Option<&ResolvedBundle> = ir.bundle.as_ref();

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    if let Some(bundle) = bundle {
        // Re-export interfaces from contracts.ts
        out.push_str("// Re-export interfaces from contracts.ts\n");
        for plugin in &bundle.plugins {
            let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
            out.push_str(&format!(
                "export {{ {plugin_var}_INTERFACE }} from './contracts';\n"
            ));
        }
    }

    out
}

fn generate_index_ts(ir: &ValidatedIr) -> String {
    let bundle: Option<&ResolvedBundle> = ir.bundle.as_ref();

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    // Barrel re-exports: the bundle entry point (polyplug_init), each plugin's
    // interface + factory from ./contracts, and any peer caller classes.
    // ImportSet merges same-source entries onto one `export { … } from '…'` line.
    let mut reexports: Vec<ImportEntry> = vec![js_reexport("polyplug_init", "./init")];
    if let Some(bundle) = bundle {
        for plugin in &bundle.plugins {
            let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
            reexports.push(js_reexport(
                &format!("{plugin_var}_INTERFACE"),
                "./contracts",
            ));
        }
        for plugin in &bundle.plugins {
            let set_factory_name: String = format!(
                "set{}Factory",
                plugin
                    .name
                    .replace(['.', '-'], "_")
                    .split('_')
                    .map(|s| {
                        let mut chars = s.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                        }
                    })
                    .collect::<String>()
            );
            reexports.push(js_reexport(&set_factory_name, "./contracts"));
        }
    }
    // Re-export peer caller classes when the bundle declares dependencies.
    let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
    for contract in &peer_contracts {
        let class_name: String = guest_contract_name_to_ts_peer(&contract.name);
        reexports.push(js_reexport(&class_name, "./peer_callers"));
    }
    out.push_str("// Main entry point for bundling\n");
    out.push_str(&js_import_block(&[&reexports]));

    out
}

fn generate_init_ts(ir: &ValidatedIr) -> String {
    let bundle: &ResolvedBundle = match ir.bundle.as_ref() {
        Some(b) => b,
        None => return String::from("// ERROR: init.ts called without bundle IR\n"),
    };

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    let contract_imports: Vec<ImportEntry> = bundle
        .plugins
        .iter()
        .map(|plugin: &ResolvedPlugin| {
            let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
            js_named(&format!("{plugin_var}_INTERFACE"), "./contracts")
        })
        .collect();
    out.push_str(&js_import_block(&[&contract_imports]));

    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("    Generic: 1,\n");
    out.push_str("    InvalidPointer: 8,\n");
    out.push_str("};\n\n");

    out.push_str("interface AbiError {\n");
    out.push_str("    code: number;\n");
    out.push_str("    message: string;\n");
    out.push_str("}\n\n");

    out.push_str("// One registration entry per implemented contract. The loader reads this\n");
    out.push_str("// array from polyplug_init's return value (nothing is deposited into any\n");
    out.push_str("// global — Rule 12) and registers one GuestContractInterface per entry.\n");
    out.push_str("interface Registration {\n");
    out.push_str("    contractLo: number;\n");
    out.push_str("    contractHi: number;\n");
    out.push_str("    interface: any;\n");
    out.push_str("    fnCount: number;\n");
    out.push_str("    contractName: string;\n");
    out.push_str("    version: number;\n");
    out.push_str("}\n\n");

    out.push_str("/**\n");
    out.push_str(" * Initialize plugin with host runtime.\n");
    out.push_str(" *\n");
    out.push_str(" * Returns `[registrations, abiError]`: the per-contract registration array\n");
    out.push_str(" * the loader consumes, plus the canonical AbiError ({ code, message }).\n");
    out.push_str(
        " * Nothing is deposited into any global namespace (Rule 12) — the loader reads\n",
    );
    out.push_str(
        " * BOTH return values. The host vtable and the `bridge` are threaded explicitly\n",
    );
    out.push_str(" * to each author factory; no host pointer or bridge is stored in any module.\n");
    out.push_str(" *\n");
    out.push_str(" * @param host_lo - HostApi pointer (low 32 bits)\n");
    out.push_str(" * @param host_hi - HostApi pointer (high 32 bits)\n");
    out.push_str(" * @param ctx_lo - BundleInitContext pointer (low 32 bits)\n");
    out.push_str(" * @param ctx_hi - BundleInitContext pointer (high 32 bits)\n");
    out.push_str(" * @param bridge - Host-capability bridge passed in by the loader\n");
    out.push_str(" */\n");
    out.push_str("export function polyplug_init(\n");
    out.push_str("    host_lo: number, host_hi: number,\n");
    out.push_str("    ctx_lo: number, ctx_hi: number,\n");
    out.push_str("    bridge: any\n");
    out.push_str("): [Registration[], AbiError] {\n");
    out.push_str("    // Validate parameters\n");
    out.push_str("    if (host_lo === 0 && host_hi === 0) {\n");
    out.push_str(
        "        return [[], { code: AbiErrorCode.Generic, message: \"null host pointer in polyplug_init\" }];\n",
    );
    out.push_str("    }\n");
    out.push_str("    if (ctx_lo === 0 && ctx_hi === 0) {\n");
    out.push_str(
        "        return [[], { code: AbiErrorCode.Generic, message: \"null ctx pointer in polyplug_init\" }];\n",
    );
    out.push_str("    }\n");
    out.push_str("    if (!bridge || !bridge.alloc) {\n");
    out.push_str(
        "        return [[], { code: AbiErrorCode.Generic, message: \"missing bridge in polyplug_init\" }];\n",
    );
    out.push_str("    }\n\n");

    out.push_str("    const registrations: Registration[] = [];\n");
    for plugin in &bundle.plugins {
        let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "    // Register plugin: {plugin_name}\n",
            plugin_name = plugin.name
        ));
        out.push_str("    registrations.push({\n");
        out.push_str(&format!(
            "        contractLo: {plugin_var}_INTERFACE.contractLo,\n"
        ));
        out.push_str(&format!(
            "        contractHi: {plugin_var}_INTERFACE.contractHi,\n"
        ));
        out.push_str(&format!("        interface: {plugin_var}_INTERFACE,\n"));
        out.push_str(&format!(
            "        fnCount: {plugin_var}_INTERFACE.fnCount,\n"
        ));
        out.push_str(&format!(
            "        contractName: {plugin_var}_INTERFACE.contractName,\n"
        ));
        out.push_str(&format!(
            "        version: {plugin_var}_INTERFACE.version,\n"
        ));
        out.push_str("    });\n\n");
    }

    out.push_str("    return [registrations, { code: AbiErrorCode.Ok, message: \"\" }];\n");
    out.push_str("}\n");

    out
}

fn generate_interface_ts_with_bindings(ir: &ValidatedIr, bindings_module: &str) -> String {
    generate_interface_ts(ir).replace("'./contracts'", &format!("'{bindings_module}'"))
}

fn generate_index_ts_with_bindings(ir: &ValidatedIr, bindings_module: &str) -> String {
    generate_index_ts(ir).replace("'./contracts'", &format!("'{bindings_module}'"))
}

fn generate_init_ts_with_bindings(ir: &ValidatedIr, bindings_module: &str) -> String {
    generate_init_ts(ir).replace("'./contracts'", &format!("'{bindings_module}'"))
}

fn generate_manifest_toml(ir: &ValidatedIr) -> String {
    let bundle: &ResolvedBundle = match ir.bundle.as_ref() {
        Some(b) => b,
        None => return String::from("# ERROR: bundle manifest called without bundle IR\n"),
    };

    let name: &str = &bundle.name;
    let version: String = format!(
        "{}.{}.{}",
        bundle.version.major, bundle.version.minor, bundle.version.patch
    );

    let mut provides: Vec<String> = bundle
        .plugins
        .iter()
        .flat_map(|p: &ResolvedPlugin| p.implements.iter().cloned())
        .map(|impl_str: String| {
            if let Some(at_pos) = impl_str.find('@') {
                let contract_name: &str = &impl_str[..at_pos];
                let version_part: &str = &impl_str[at_pos + 1..];
                if let Some(dot_pos) = version_part.find('.') {
                    let major: &str = &version_part[..dot_pos];
                    format!("{}@{}", contract_name, major)
                } else {
                    impl_str
                }
            } else {
                impl_str
            }
        })
        .collect();
    provides.sort();
    provides.dedup();

    let provides_toml: String = if provides.is_empty() {
        String::from("[]")
    } else {
        format!(
            "[{}]",
            provides
                .iter()
                .map(|s: &String| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let provides_set: HashSet<String> = provides.iter().cloned().collect();
    let fn_count_entries: Vec<String> = ir
        .contracts
        .iter()
        .filter(|c: &&ResolvedContract| {
            provides_set.contains(&format!("{}@{}", c.name, c.version.major))
        })
        .map(|c: &ResolvedContract| {
            let fn_count: u32 = c.functions.len() as u32;
            format!("\"{}@{}\" = {}", c.name, c.version.major, fn_count)
        })
        .collect();
    let function_count_toml: String = format!("{{ {} }}", fn_count_entries.join(", "));

    let reinit: bool = bundle.needs_reinit_on_dep_reload;
    let file_field: String = super::format_manifest_file_field(&bundle.file);
    let loader: &str = &bundle.loader;

    let dep_toml: String = super::emit_manifest_dependencies(&bundle.dependencies);

    format!(
        "# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n\
         name = \"{name}\"\n\
         id = {bundle_id}\n\
         version = \"{version}\"\n\
         loader = \"{loader}\"\n\
         provides = {provides_toml}\n\
         function_count = {function_count_toml}\n\
         needs_reinit_on_dep_reload = {reinit}\n\
         {file_field}\n\
         {dep_toml}",
        bundle_id = bundle.bundle_id
    )
}

fn generate_readme_quickjs(ir: &ValidatedIr) -> String {
    let bundle_name: &str = ir
        .contracts
        .first()
        .map(|c: &ResolvedContract| c.name.as_str())
        .unwrap_or("my-bundle");
    format!(
        "# js-quickjs Plugin Bundle: {bundle_name}\n\n\
         ## Requirements\n\
         - rolldown: `npm i -g rolldown`\n\n\
         ## Build\n\
         ```bash\n\
         rolldown index.ts --format iife --platform neutral --file bundle.js\n\
         ```\n"
    )
}

fn generate_callers_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    generate_callers_ts_with_direct_handles(ir, false)
}

fn generate_internal_callers_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    generate_callers_ts_with_direct_handles(ir, true)
}

fn generate_callers_ts_with_direct_handles(
    ir: &ValidatedIr,
    direct_handles: bool,
) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js (Deno host callers over the polyplug Deno FFI SDK)\n\n",
    );
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");

    // The host callers run under Deno against the polyplug Deno FFI host SDK.
    // They resolve a contract via the runtime, decode the raw
    // `GuestContractInterface*` through the SDK's `GuestContractInterfaceView`,
    // and dispatch directly through the interface function pointers (native or VM).
    //
    // The SDK types are declared structurally here (rather than imported from the
    // 'polyplug' module) so the generated file type-checks standalone under
    // `deno check` regardless of the host's import map / directory depth. At
    // runtime the caller is constructed with the real SDK `Runtime` instance.
    out.push_str("// Structural SDK types (see sdks/js/host/polyplug/mod.js).\n");
    out.push_str("interface GuestContractInterfaceView {\n");
    out.push_str("    interfacePtr(): Deno.PointerValue;\n");
    out.push_str("    isValid(): boolean;\n");
    out.push_str("    functionCount(): number;\n");
    out.push_str("    createInstance(): Uint8Array;\n");
    out.push_str("    destroyInstance(instance: Uint8Array): void;\n");
    out.push_str(
        "    dispatch(slot: number, instance: Uint8Array, argsPtr: Deno.PointerValue, outPtr: Deno.PointerValue): number;\n",
    );
    out.push_str("}\n");
    out.push_str("interface Runtime {\n");
    out.push_str("    findGuestContract(contractId: bigint, minVersion?: number): number;\n");
    out.push_str(
        "    resolveGuestContractInterface(handle: number): GuestContractInterfaceView | null;\n",
    );
    out.push_str("    registryRevision(): bigint;\n");
    out.push_str("    alloc(size: number, align?: number): Deno.PointerValue;\n");
    out.push_str("    free(ptr: Deno.PointerValue, size: number, align?: number): void;\n");
    out.push_str("}\n\n");

    // ABI error codes (match polyplug_abi.AbiErrorCode)
    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("export const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("    Generic: 1,\n");
    out.push_str("    InvalidPointer: 8,\n");
    out.push_str("} as const;\n\n");

    // Argument/return marshalling allocates StringView/Buffer payloads through
    // the host allocator and packs values into C-layout buffers (see the per-
    // method callers below).
    out.push_str("const _encoder = new TextEncoder();\n");
    out.push_str("const _decoder = new TextDecoder();\n\n");

    // Contract ID constants (full u64 as bigint, matching the SDK's findGuestContract).
    out.push_str("// Contract ID constants (FNV-1a of \"guest_contract:name@major\")\n");
    for contract in &ir.contracts {
        let upper_name: String = contract.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "/** Contract ID for `{}@{}` */\n",
            contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "export const {}_CONTRACT_ID = 0x{:016X}n;\n",
            upper_name, contract.contract_id
        ));
    }
    out.push('\n');

    for contract in &ir.contracts {
        generate_host_caller_class_quickjs(&mut out, ir, contract, direct_handles)?;
    }

    Ok(out)
}

fn generate_host_caller_class_quickjs(
    out: &mut String,
    ir: &ValidatedIr,
    contract: &ResolvedContract,
    direct_handles: bool,
) -> Result<(), PolyplugcError> {
    let class_name: String = contract_to_class_name(&contract.name);
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let contract_id_const: String = format!("{}_CONTRACT_ID", contract_upper);

    emit_js_attributes(out, CustomizableNode::GuestContract, &contract.langs, "");
    if let Some(docs) = contract.docs.as_deref() {
        let caller_docs: String = format!(
            "Host caller for contract `{}` over the Deno FFI SDK.\n\n{docs}",
            contract.name
        );
        write_jsdoc(out, "", Some(&caller_docs), &[], None);
    } else {
        out.push_str(&format!(
            "/** Host caller for contract `{}` over the Deno FFI SDK. */\n",
            contract.name
        ));
    }
    out.push_str(&format!("export class {}Contract {{\n", class_name));
    out.push_str("    #rt: Runtime;\n");
    out.push_str("    #view: GuestContractInterfaceView | null;\n");
    out.push_str("    #instance: Uint8Array;\n");
    // Retained so the cache can re-resolve after a hot-reload (which swaps a new
    // interface into the same slot) or report a gone contract after an unload.
    out.push_str("    #handle: number;\n");
    // Revision value read when the interface was resolved. Compared before each
    // dispatch against the live counter to detect a reload/unload and re-resolve,
    // so the cached interface pointer and instance never dangle.
    out.push_str("    #cachedRevision: bigint;\n\n");
    out.push_str("    #destroyed: boolean;\n\n");

    out.push_str(
        "    private constructor(rt: Runtime, view: GuestContractInterfaceView, instance: Uint8Array, handle: number, cachedRevision: bigint) {\n",
    );
    out.push_str("        this.#rt = rt;\n");
    out.push_str("        this.#view = view;\n");
    out.push_str("        this.#instance = instance;\n");
    out.push_str("        this.#handle = handle;\n");
    out.push_str("        this.#cachedRevision = cachedRevision;\n");
    out.push_str("        this.#destroyed = false;\n");
    out.push_str("    }\n\n");

    out.push_str(
        "    /** Resolve the contract and create an instance, or null if unavailable. */\n",
    );
    out.push_str(&format!(
        "    static create(rt: Runtime): {}Contract | null {{\n",
        class_name
    ));
    out.push_str(&format!(
        "        const handle = rt.findGuestContract({}, 0);\n",
        contract_id_const
    ));
    out.push_str("        const view = rt.resolveGuestContractInterface(handle);\n");
    // Validity keys off the interface pointer, never off instance data.
    out.push_str("        if (view === null || !view.isValid()) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    // A null instance.data is a VALID dispatch token for stateless contracts.
    out.push_str("        const instance = view.createInstance();\n");
    // Fetch the registry revision counter ONCE and read its current value, so every
    // later call can detect a reload/unload with a direct atomic load and re-resolve
    // before dispatching through a stale interface.
    out.push_str("        const cachedRevision = rt.registryRevision();\n");
    out.push_str(&format!(
        "        return new {}Contract(rt, view, instance, handle, cachedRevision);\n",
        class_name
    ));
    out.push_str("    }\n\n");
    if direct_handles {
        out.push_str(&format!(
            "    /** Construct from this registration's exact committed handle. */\n    static createFromHandle(rt: Runtime, handle: number): {}Contract | null {{\n",
            class_name
        ));
        out.push_str("        const view = rt.resolveGuestContractInterface(handle);\n");
        out.push_str("        if (view === null || !view.isValid()) {\n");
        out.push_str("            return null;\n");
        out.push_str("        }\n");
        out.push_str("        const instance = view.createInstance();\n");
        out.push_str("        const cachedRevision = rt.registryRevision();\n");
        out.push_str(&format!(
            "        return new {}Contract(rt, view, instance, handle, cachedRevision);\n",
            class_name
        ));
        out.push_str("    }\n\n");
    }

    // Read the registry revision through the cached pointer — one atomic load, no call
    // into the runtime. Returns the cached value ("unchanged") when there is no counter.
    out.push_str("    #liveRevision(): bigint {\n");
    out.push_str("        return this.#rt.registryRevision();\n");
    out.push_str("    }\n\n");

    out.push_str("    #invalidate(): void {\n");
    out.push_str("        this.#view = null;\n");
    out.push_str("        this.#instance = new Uint8Array(0);\n");
    out.push_str("        this.#destroyed = true;\n");
    out.push_str("    }\n\n");
    // Re-resolve the cached interface after the registry changed under us. An unrelated
    // change can retain the same interface; in that case preserve the stateful instance
    // and only refresh the revision. A hot-reload instead resolves a new interface, whose
    // old instance was epoch-reclaimed and must be abandoned before replacement.
    out.push_str("    #revalidate(): boolean {\n");
    out.push_str("        if (this.#destroyed || this.#view === null) {\n");
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str("        const view = this.#rt.resolveGuestContractInterface(this.#handle);\n");
    out.push_str("        if (view === null || !view.isValid()) {\n");
    out.push_str("            this.#invalidate();\n");
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str("        if (view.interfacePtr() === this.#view.interfacePtr()) {\n");
    out.push_str("            this.#cachedRevision = this.#liveRevision();\n");
    out.push_str("            return true;\n");
    out.push_str("        }\n");
    out.push_str("        this.#view = view;\n");
    out.push_str("        this.#instance = view.createInstance();\n");
    out.push_str("        this.#cachedRevision = this.#liveRevision();\n");
    out.push_str("        return true;\n");
    out.push_str("    }\n\n");

    out.push_str("    /** True while this caller retains a live resolved interface. */\n");
    out.push_str("    isValid(): boolean {\n");
    out.push_str("        if (this.#destroyed || this.#view === null) {\n");
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str(
        "        if (this.#liveRevision() !== this.#cachedRevision && !this.#revalidate()) {\n",
    );
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str("        return this.#view !== null && this.#view.isValid();\n");
    out.push_str("    }\n\n");

    out.push_str("    /** Destroy this instance exactly once. */\n");
    out.push_str("    destroy(): void {\n");
    out.push_str("        if (this.#destroyed || this.#view === null) {\n");
    out.push_str("            throw new Error('caller has already been destroyed');\n");
    out.push_str("        }\n");
    out.push_str("        const cachedView = this.#view;\n");
    out.push_str("        if (cachedView === null) {\n");
    out.push_str("            throw new Error('caller has already been destroyed');\n");
    out.push_str("        }\n");
    out.push_str("        if (this.#liveRevision() !== this.#cachedRevision) {\n");
    out.push_str(
        "            const resolved = this.#rt.resolveGuestContractInterface(this.#handle);\n",
    );
    out.push_str("            if (resolved === null || !resolved.isValid() || resolved.interfacePtr() !== cachedView.interfacePtr()) {\n");
    out.push_str("                this.#invalidate();\n");
    out.push_str("                throw new Error('caller cannot be destroyed after contract reload/unload');\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        const instance = this.#instance;\n");
    out.push_str("        this.#invalidate();\n");
    out.push_str("        cachedView.destroyInstance(instance);\n");
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_host_caller_method_deno(out, ir, func)?;
    }

    out.push_str("}\n\n");
    Ok(())
}

/// Generate one Deno host-caller method with full ABI marshalling.
///
/// Supports every caller shape the IR can express: primitives (incl. `u64`/`i64`
/// as `bigint`), `bool`, enums (read and written at their repr width, UNSIGNED),
/// `StringView` (`string`), `Buffer` (`Uint8Array`), and one-level structs whose
/// fields are any of the above. Arguments are packed into a single C-layout
/// struct buffer; the return value is read back from a C-layout out buffer.
/// `StringView`/`Buffer` payloads (arguments and struct fields) are allocated
/// through the host allocator and freed after dispatch; returned
/// `StringView`/`Buffer` payloads are copied out and freed.
fn generate_host_caller_method_deno(
    out: &mut String,
    ir: &ValidatedIr,
    func: &ResolvedFunction,
) -> Result<(), PolyplugcError> {
    let fn_id: u32 = func.function_id;

    let mut param_decls: Vec<String> = Vec::with_capacity(func.params.len());
    for param in &func.params {
        let ts_ty: String = deno_caller_ts_type(&param.ty, ir)?;
        param_decls.push(format!("{}: {}", param.name, ts_ty));
    }
    let ret_ts: String = match &func.returns {
        Some(ty) => deno_caller_ts_type(ty, ir)?,
        None => "void".to_owned(),
    };

    emit_js_function_attributes(out, func, "    ");
    if func.docs.is_some()
        || func.params.iter().any(|param| param.docs.is_some())
        || func.return_docs.is_some()
    {
        let documented_params: Vec<(&str, Option<&str>)> = func
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.docs.as_deref()))
            .collect();
        write_jsdoc(
            out,
            "    ",
            func.docs.as_deref(),
            &documented_params,
            func.return_docs.as_deref(),
        );
    } else {
        out.push_str(&format!("    /** Call `{}` */\n", func.name));
    }
    out.push_str(&format!(
        "    {}({}): {} {{\n",
        func.name,
        param_decls.join(", "),
        ret_ts
    ));

    // Cheap per-call staleness check: read the registry revision through the cached
    // pointer. While it matches the value cached when this caller resolved, the
    // interface pointer is current and we dispatch directly; on any change (hot-reload
    // or unload) we re-resolve first, so the cached pointer is never used once it
    // dangles. A failed re-resolve means the contract is gone — throw NotFound.
    out.push_str(
        "        if (this.#destroyed || this.#view === null || (this.#liveRevision() !== this.#cachedRevision && !this.#revalidate())) {\n",
    );
    out.push_str(&format!(
        "            throw new Error('call `{}` failed: contract gone after reload/unload');\n",
        func.name
    ));
    out.push_str("        }\n");
    out.push_str("        const view = this.#view;\n");
    out.push_str("        if (view === null) {\n");
    out.push_str(&format!(
        "            throw new Error('call `{}` failed: caller has been destroyed');\n",
        func.name
    ));
    out.push_str("        }\n");

    // Validate the function index against the interface's reported function count.
    // VM-dispatch interfaces report a count of 0 (the VM routes by fn_id itself),
    // so only enforce the bound for native interfaces that report a real count.
    out.push_str(&format!(
        "        if (view.functionCount() > 0 && {fn_id} >= view.functionCount()) {{\n"
    ));
    out.push_str(&format!(
        "            throw new Error('function `{}` not available in interface');\n",
        func.name
    ));
    out.push_str("        }\n");

    out.push_str("        const rt = this.#rt;\n");
    // Every host_alloc made while packing arguments, freed after dispatch.
    out.push_str("        const _allocs: [Deno.PointerValue, number][] = [];\n");

    // Pack arguments into a single C-layout struct buffer.
    let args_size: usize = deno_args_total_size(func, ir)?;
    out.push_str(&format!(
        "        const argsBuf = new Uint8Array({});\n",
        args_size.max(1)
    ));
    if !func.params.is_empty() {
        out.push_str("        const argsDv = new DataView(argsBuf.buffer);\n");
        let mut offset: usize = 0;
        let mut alloc_idx: u32 = 0;
        for param in &func.params {
            let align: usize = js_c_align(&param.ty, ir)?;
            offset = align_up(offset, align);
            emit_deno_write_value(
                out,
                &param.ty,
                "argsDv",
                offset,
                &param.name,
                ir,
                &mut alloc_idx,
            )?;
            offset += js_c_size(&param.ty, ir)?;
        }
    }
    out.push_str("        const argsPtr = Deno.UnsafePointer.of(argsBuf);\n");

    // Out buffer sized to the return type (or one byte for void).
    let ret_size: usize = match &func.returns {
        Some(ty) => js_c_size(ty, ir)?,
        None => 0,
    };
    out.push_str(&format!(
        "        const outBuf = new Uint8Array({});\n",
        ret_size.max(1)
    ));
    out.push_str("        const outPtr = Deno.UnsafePointer.of(outBuf);\n");

    // Dispatch through the resolved interface (native or VM).
    out.push_str(&format!(
        "        const code = view.dispatch({fn_id}, this.#instance, argsPtr, outPtr);\n"
    ));
    // Release argument payloads regardless of outcome.
    out.push_str("        for (const [_p, _s] of _allocs) { rt.free(_p, _s, 1); }\n");
    out.push_str("        if (code !== AbiErrorCode.Ok) {\n");
    out.push_str(&format!(
        "            throw new Error('call `{}` failed: AbiError code ' + code);\n",
        func.name
    ));
    out.push_str("        }\n");

    if let Some(ret_ty) = &func.returns {
        out.push_str("        const outDv = new DataView(outBuf.buffer);\n");
        let mut read_idx: u32 = 0;
        // The caller OWNS the guest's return payload (the guest host-allocated it
        // for us), so the read frees it after copying out.
        let local: String = emit_deno_read_local(out, ret_ty, "outDv", 0, ir, &mut read_idx, true)?;
        out.push_str(&format!("        return {local};\n"));
    }

    out.push_str("    }\n\n");
    Ok(())
}

/// Ergonomic TypeScript type for a Deno host-caller parameter or return value.
/// Enums travel as their numeric repr value (`number`); structs become an inline
/// object type so the generated file stays import-free and `deno check`-clean.
fn deno_caller_ts_type(ty: &ResolvedTypeRef, ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8
            | PrimitiveType::U16
            | PrimitiveType::U32
            | PrimitiveType::I8
            | PrimitiveType::I16
            | PrimitiveType::I32
            | PrimitiveType::F32
            | PrimitiveType::F64 => "number".to_owned(),
            PrimitiveType::Bool => "boolean".to_owned(),
            PrimitiveType::U64 | PrimitiveType::I64 => "bigint".to_owned(),
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok("string".to_owned()),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok("Uint8Array".to_owned()),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok("bigint".to_owned()),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok("void".to_owned()),
        ResolvedTypeRef::UserDefined(name) => {
            if js_enum_for_type(ty, &ir.enums).is_some() {
                return Ok("number".to_owned());
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut fields: Vec<String> = Vec::with_capacity(s.fields.len());
                for field in &s.fields {
                    // Struct-typed fields recurse into a nested inline object type
                    // ({ a: { b: number } }); the C layout is computed by
                    // js_c_size/js_c_align, so nesting depth is unbounded.
                    let field_ts: String = deno_caller_ts_type(&field.ty, ir)?;
                    fields.push(format!("{}: {}", field.name, field_ts));
                }
                return Ok(format!("{{ {} }}", fields.join("; ")));
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Total C-layout size of a function's argument struct (natural alignment, tail
/// padded to the struct's own alignment) — matches every other host caller.
fn deno_args_total_size(
    func: &ResolvedFunction,
    ir: &ValidatedIr,
) -> Result<usize, PolyplugcError> {
    let mut offset: usize = 0;
    let mut max_align: usize = 1;
    for param in &func.params {
        let align: usize = js_c_align(&param.ty, ir)?;
        if align > max_align {
            max_align = align;
        }
        offset = align_up(offset, align);
        offset += js_c_size(&param.ty, ir)?;
    }
    Ok(align_up(offset, max_align))
}

/// Emit statements writing `value` (a JS expression of `ty`'s ergonomic shape)
/// into `dv` at `off` using Deno-FFI primitives. StringView/Buffer payloads are
/// host-allocated and pushed onto `_allocs` for later release; structs recurse
/// field-by-field to unbounded nesting depth. Integers/enums are written at their
/// exact repr width (enums UNSIGNED).
fn emit_deno_write_value(
    out: &mut String,
    ty: &ResolvedTypeRef,
    dv: &str,
    off: usize,
    value: &str,
    ir: &ValidatedIr,
    alloc_idx: &mut u32,
) -> Result<(), PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => {
            match p {
                PrimitiveType::U8 => out.push_str(&format!(
                    "        {dv}.setUint8({off}, Number({value}) & 0xFF);\n"
                )),
                PrimitiveType::I8 => {
                    out.push_str(&format!("        {dv}.setInt8({off}, Number({value}));\n"))
                }
                PrimitiveType::U16 => out.push_str(&format!(
                    "        {dv}.setUint16({off}, Number({value}) & 0xFFFF, true);\n"
                )),
                PrimitiveType::I16 => out.push_str(&format!(
                    "        {dv}.setInt16({off}, Number({value}), true);\n"
                )),
                PrimitiveType::U32 => out.push_str(&format!(
                    "        {dv}.setUint32({off}, Number({value}) >>> 0, true);\n"
                )),
                PrimitiveType::I32 => out.push_str(&format!(
                    "        {dv}.setInt32({off}, Number({value}) | 0, true);\n"
                )),
                PrimitiveType::F32 => out.push_str(&format!(
                    "        {dv}.setFloat32({off}, Number({value}), true);\n"
                )),
                PrimitiveType::F64 => out.push_str(&format!(
                    "        {dv}.setFloat64({off}, Number({value}), true);\n"
                )),
                PrimitiveType::Bool => {
                    out.push_str(&format!("        {dv}.setUint8({off}, {value} ? 1 : 0);\n"))
                }
                PrimitiveType::U64 => out.push_str(&format!(
                    "        {dv}.setBigUint64({off}, BigInt({value}), true);\n"
                )),
                PrimitiveType::I64 => out.push_str(&format!(
                    "        {dv}.setBigInt64({off}, BigInt({value}), true);\n"
                )),
            }
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(&format!(
                "        {dv}.setBigUint64({off}, BigInt({value}), true);\n"
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            emit_deno_write_stringview(out, dv, off, value, alloc_idx);
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            emit_deno_write_buffer(out, dv, off, value, alloc_idx);
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(()),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                match e.repr {
                    ReprType::U8 => out.push_str(&format!(
                        "        {dv}.setUint8({off}, Number({value}) & 0xFF);\n"
                    )),
                    ReprType::U16 => out.push_str(&format!(
                        "        {dv}.setUint16({off}, Number({value}) & 0xFFFF, true);\n"
                    )),
                    ReprType::U32 => out.push_str(&format!(
                        "        {dv}.setUint32({off}, Number({value}) >>> 0, true);\n"
                    )),
                    ReprType::U64 => out.push_str(&format!(
                        "        {dv}.setBigUint64({off}, BigInt({value}), true);\n"
                    )),
                }
                return Ok(());
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut field_off: usize = off;
                for field in &s.fields {
                    let align: usize = js_c_align(&field.ty, ir)?;
                    field_off = align_up(field_off, align);
                    let field_value: String = format!("{value}.{}", field.name);
                    emit_deno_write_value(
                        out,
                        &field.ty,
                        dv,
                        field_off,
                        &field_value,
                        ir,
                        alloc_idx,
                    )?;
                    field_off += js_c_size(&field.ty, ir)?;
                }
                return Ok(());
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Host-allocate a StringView payload, copy `value` into it, write the
/// `{ ptr, len }` slot at `off`, and record the allocation in `_allocs`.
fn emit_deno_write_stringview(out: &mut String, dv: &str, off: usize, value: &str, idx: &mut u32) {
    let n: u32 = *idx;
    *idx += 1;
    out.push_str(&format!(
        "        const _sv{n}Bytes = _encoder.encode({value});\n"
    ));
    out.push_str(&format!(
        "        const _sv{n}Alloc = _sv{n}Bytes.length > 0 ? _sv{n}Bytes.length : 1;\n"
    ));
    out.push_str(&format!(
        "        const _sv{n}Ptr = rt.alloc(_sv{n}Alloc, 1);\n"
    ));
    out.push_str(&format!(
        "        if (_sv{n}Ptr === null) {{ throw new Error('host_alloc failed'); }}\n"
    ));
    out.push_str(&format!(
        "        if (_sv{n}Bytes.length > 0) {{ new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_sv{n}Ptr, _sv{n}Bytes.length)).set(_sv{n}Bytes); }}\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({off}, BigInt(Deno.UnsafePointer.value(_sv{n}Ptr)), true);\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({}, BigInt(_sv{n}Bytes.length), true);\n",
        off + 8
    ));
    out.push_str(&format!(
        "        _allocs.push([_sv{n}Ptr, _sv{n}Alloc]);\n"
    ));
}

/// Host-allocate a Buffer payload, copy `value` (a Uint8Array) into it, write the
/// `{ ptr, len, cap }` slot at `off` (cap == len), and record the allocation.
fn emit_deno_write_buffer(out: &mut String, dv: &str, off: usize, value: &str, idx: &mut u32) {
    let n: u32 = *idx;
    *idx += 1;
    out.push_str(&format!("        const _bf{n}Data = {value};\n"));
    out.push_str(&format!(
        "        const _bf{n}Alloc = _bf{n}Data.length > 0 ? _bf{n}Data.length : 1;\n"
    ));
    out.push_str(&format!(
        "        const _bf{n}Ptr = rt.alloc(_bf{n}Alloc, 1);\n"
    ));
    out.push_str(&format!(
        "        if (_bf{n}Ptr === null) {{ throw new Error('host_alloc failed'); }}\n"
    ));
    out.push_str(&format!(
        "        if (_bf{n}Data.length > 0) {{ new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_bf{n}Ptr, _bf{n}Data.length)).set(_bf{n}Data); }}\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({off}, BigInt(Deno.UnsafePointer.value(_bf{n}Ptr)), true);\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({}, BigInt(_bf{n}Data.length), true);\n",
        off + 8
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({}, BigInt(_bf{n}Data.length), true);\n",
        off + 16
    ));
    out.push_str(&format!(
        "        _allocs.push([_bf{n}Ptr, _bf{n}Alloc]);\n"
    ));
}

/// Emit statements reading one ABI value of `ty` from `dv` at `off` into a
/// freshly-declared local, returning that local's name. Scalars/enums read at
/// their exact repr width (enums UNSIGNED); StringView/Buffer payloads are copied
/// out and, when `owns_payload`, freed; structs recurse field-by-field into a
/// nested object literal to unbounded depth.
// Mirrors emit_deno_write_value's irreducible marshalling context (out, type,
// view, offset, ir, counter) plus the ownership flag.
fn emit_deno_read_local(
    out: &mut String,
    ty: &ResolvedTypeRef,
    dv: &str,
    off: usize,
    ir: &ValidatedIr,
    idx: &mut u32,
    owns_payload: bool,
) -> Result<String, PolyplugcError> {
    let n: u32 = *idx;
    *idx += 1;
    let name: String = format!("_r{n}");
    match ty {
        ResolvedTypeRef::Primitive(p) => {
            let expr: String = match p {
                PrimitiveType::U8 => format!("{dv}.getUint8({off})"),
                PrimitiveType::I8 => format!("{dv}.getInt8({off})"),
                PrimitiveType::U16 => format!("{dv}.getUint16({off}, true)"),
                PrimitiveType::I16 => format!("{dv}.getInt16({off}, true)"),
                PrimitiveType::U32 => format!("{dv}.getUint32({off}, true)"),
                PrimitiveType::I32 => format!("{dv}.getInt32({off}, true)"),
                PrimitiveType::F32 => format!("{dv}.getFloat32({off}, true)"),
                PrimitiveType::F64 => format!("{dv}.getFloat64({off}, true)"),
                PrimitiveType::Bool => format!("({dv}.getUint8({off}) !== 0)"),
                PrimitiveType::U64 => format!("{dv}.getBigUint64({off}, true)"),
                PrimitiveType::I64 => format!("{dv}.getBigInt64({off}, true)"),
            };
            out.push_str(&format!("        const {name} = {expr};\n"));
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(&format!(
                "        const {name} = {dv}.getBigUint64({off}, true);\n"
            ));
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            emit_deno_read_stringview(out, dv, off, &name, owns_payload);
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            emit_deno_read_buffer(out, dv, off, &name, owns_payload);
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Err(PolyplugcError::UnsupportedType {
            type_name: "void".to_owned(),
            lang: "js-quickjs".to_owned(),
        }),
        ResolvedTypeRef::UserDefined(type_name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                let expr: String = match e.repr {
                    ReprType::U8 => format!("{dv}.getUint8({off})"),
                    ReprType::U16 => format!("{dv}.getUint16({off}, true)"),
                    ReprType::U32 => format!("{dv}.getUint32({off}, true)"),
                    ReprType::U64 => format!("Number({dv}.getBigUint64({off}, true))"),
                };
                out.push_str(&format!("        const {name} = {expr};\n"));
                return Ok(name);
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut field_off: usize = off;
                let mut members: Vec<String> = Vec::with_capacity(s.fields.len());
                for field in &s.fields {
                    let align: usize = js_c_align(&field.ty, ir)?;
                    field_off = align_up(field_off, align);
                    let field_local: String =
                        emit_deno_read_local(out, &field.ty, dv, field_off, ir, idx, owns_payload)?;
                    members.push(format!("{}: {field_local}", field.name));
                    field_off += js_c_size(&field.ty, ir)?;
                }
                out.push_str(&format!(
                    "        const {name} = {{ {} }};\n",
                    members.join(", ")
                ));
                return Ok(name);
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: type_name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Read a StringView at `off`, decode it to `name` (a `let` string). Null/empty
/// views decode to `''`. `owns_payload` encodes WHO owns the pointed-to bytes:
/// when true the reader owns them and frees them after copying (a caller reading a
/// guest RETURN it host-allocated for us); when false the reader only borrows them
/// and must not free (a host-contract PROVIDER reading a caller-owned ARG — freeing
/// it would be a use-after-free).
fn emit_deno_read_stringview(
    out: &mut String,
    dv: &str,
    off: usize,
    name: &str,
    owns_payload: bool,
) {
    out.push_str(&format!("        let {name} = '';\n"));
    out.push_str("        {\n");
    out.push_str(&format!(
        "            const _p = {dv}.getBigUint64({off}, true);\n"
    ));
    out.push_str(&format!(
        "            const _l = Number({dv}.getBigUint64({}, true));\n",
        off + 8
    ));
    out.push_str("            if (_p !== 0n && _l > 0) {\n");
    out.push_str("                const _ptr = Deno.UnsafePointer.create(_p);\n");
    out.push_str("                if (_ptr !== null) {\n");
    out.push_str(&format!(
        "                    {name} = _decoder.decode(new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_ptr, _l)).slice());\n"
    ));
    if owns_payload {
        out.push_str("                    rt.free(_ptr, _l, 1);\n");
    }
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
}

/// Read a Buffer at `off`, copy it into `name` (a `let` Uint8Array). Null/empty
/// buffers decode to an empty array. `owns_payload` follows the same ownership
/// rule as `emit_deno_read_stringview` (own a guest RETURN → free by capacity;
/// borrow a caller-owned ARG → leave intact).
fn emit_deno_read_buffer(out: &mut String, dv: &str, off: usize, name: &str, owns_payload: bool) {
    out.push_str(&format!("        let {name} = new Uint8Array(0);\n"));
    out.push_str("        {\n");
    out.push_str(&format!(
        "            const _p = {dv}.getBigUint64({off}, true);\n"
    ));
    out.push_str(&format!(
        "            const _l = Number({dv}.getBigUint64({}, true));\n",
        off + 8
    ));
    out.push_str(&format!(
        "            const _c = Number({dv}.getBigUint64({}, true));\n",
        off + 16
    ));
    out.push_str("            if (_p !== 0n && _l > 0) {\n");
    out.push_str("                const _ptr = Deno.UnsafePointer.create(_p);\n");
    out.push_str("                if (_ptr !== null) {\n");
    out.push_str(&format!(
        "                    {name} = new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_ptr, _l)).slice();\n"
    ));
    if owns_payload {
        out.push_str("                    rt.free(_ptr, _c > 0 ? _c : _l, 1);\n");
    }
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
}

fn contract_to_class_name(contract_name: &str) -> String {
    canonical_pascal_case(contract_name)
}

// ─── Host Contract Interface Generation ────────────────────────────────────────

/// Convert host contract name to TypeScript interface name.
/// e.g. "host.logger" -> "HostLogger", "host.fs.reader" -> "HostFsReader"
fn host_contract_name_to_ts_interface(name: &str) -> String {
    let name_without_prefix: &str = name.strip_prefix("host.").unwrap_or(name);

    let pascal: String = canonical_pascal_case(name_without_prefix);

    if pascal.starts_with("Host") {
        pascal
    } else {
        "Host".to_owned() + &pascal
    }
}

/// Generate ergonomic TypeScript type name for host interface method parameters.
/// For host interfaces, we use ergonomic TypeScript types:
/// - StringView -> string (more ergonomic for host implementers)
/// - Buffer -> Uint8Array (more ergonomic for host implementers)
/// - UserDefined -> TypeName (passed by reference in TypeScript)
/// - Primitives -> number (u32, i32, etc.) or { lo: number; hi: number } (u64, i64)
fn ts_host_param_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => ts_primitive_host(p).to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Uint8Array".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "{ lo: number; hi: number }".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Generate TypeScript return type name for host interface methods.
/// Return types are owned where appropriate:
/// - StringView -> string (owned)
/// - Buffer -> Uint8Array (owned)
/// - UserDefined -> TypeName (owned)
/// - Primitives -> number or { lo: number; hi: number }
fn ts_host_return_type(ty: &ResolvedTypeRef) -> String {
    ts_host_param_type(ty) // Same mapping for params and returns
}

/// TypeScript primitive type for host interfaces (ergonomic, not ABI-level).
fn ts_primitive_host(p: &PrimitiveType) -> &'static str {
    match p {
        PrimitiveType::U8
        | PrimitiveType::U16
        | PrimitiveType::U32
        | PrimitiveType::I8
        | PrimitiveType::I16
        | PrimitiveType::I32
        | PrimitiveType::F32
        | PrimitiveType::F64 => "number",
        PrimitiveType::Bool => "boolean",
        PrimitiveType::U64 | PrimitiveType::I64 => "{ lo: number; hi: number }",
    }
}

/// Generate one interface method for a host contract function.
fn generate_ts_host_interface_method(out: &mut String, func: &ResolvedFunction) {
    let return_type: String = match &func.returns {
        Some(ty) => ts_host_return_type(ty),
        None => "void".to_owned(),
    };

    let method_name: String = func
        .name
        .split(['_', '.'])
        .filter(|seg: &&str| !seg.is_empty())
        .map(|seg: &str| {
            let mut c: core::str::Chars<'_> = seg.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("");

    let params_str: String = if func.params.is_empty() {
        String::new()
    } else {
        func.params
            .iter()
            .map(|p: &ResolvedParam| {
                let ts_ty: String = ts_host_param_type(&p.ty);
                format!("{}: {}", p.name, ts_ty)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let documented_params: Vec<(&str, Option<&str>)> = func
        .params
        .iter()
        .map(|param| (param.name.as_str(), param.docs.as_deref()))
        .collect();
    emit_js_function_attributes(out, func, "    ");
    write_jsdoc(
        out,
        "    ",
        func.docs.as_deref(),
        &documented_params,
        func.return_docs.as_deref(),
    );
    out.push_str(&format!(
        "    {}({}): {};\n",
        method_name, params_str, return_type
    ));
}

/// Generate the interface definition for one host contract.
fn generate_ts_host_contract_interface(out: &mut String, contract: &ResolvedHostContract) {
    let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
    emit_js_attributes(out, CustomizableNode::HostContract, &contract.langs, "");
    out.push_str(&format!(
        "/**\n * Host interface for contract `{}` (id=0x{:016X})\n * Hosts implement this interface to provide functionality to plugins.\n */\n",
        contract.name, contract.contract_id
    ));
    write_jsdoc(out, "", contract.docs.as_deref(), &[], None);
    out.push_str(&format!("export interface {} {{\n", iface_name));

    for func in &contract.functions {
        generate_ts_host_interface_method(out, func);
    }

    out.push_str("}\n\n");
}

/// Generate `host/contracts.ts` — host interfaces for each host contract.
fn generate_host_contracts_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side interfaces)\n\n",
    );
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");

    for contract in &ir.host_contracts {
        generate_ts_host_contract_interface(&mut out, contract);
    }

    // Emit contract ID constants
    out.push_str("// Contract ID constants\n");
    for contract in &ir.host_contracts {
        let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
        let const_name: String = iface_name.to_uppercase() + "_CONTRACT_ID";
        out.push_str(&format!(
            "/** Contract ID for `{}` (FNV-1a of \"host_contract:{}@{}\") */\n",
            contract.name, contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "export const {} = 0x{:016X}n;\n\n",
            const_name, contract.contract_id
        ));
    }

    out
}

// ─── Guest Host Contract Caller Generation ────────────────────────────────────────

/// Convert host contract name to TypeScript guest caller class name.
/// e.g. "host.logger" -> "HostLoggerContract", "host.fs.reader" -> "HostFsReaderContract"
fn host_contract_name_to_ts_caller(name: &str) -> String {
    let name_without_prefix: &str = name.strip_prefix("host.").unwrap_or(name);

    let pascal: String = canonical_pascal_case(name_without_prefix);

    if pascal.starts_with("Host") {
        pascal + "Contract"
    } else {
        "Host".to_owned() + &pascal + "Contract"
    }
}

/// Generate ergonomic TypeScript type name for guest caller method parameters.
/// For guest callers, we use ergonomic TypeScript types:
/// - StringView -> string (converted to StringView at ABI boundary)
/// - Buffer -> Uint8Array (converted to Buffer at ABI boundary)
/// - UserDefined -> TypeName (passed by reference)
/// - Primitives -> number (u32, i32, etc.) or { lo: number; hi: number } (u64, i64)
fn ts_guest_caller_param_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => ts_primitive_guest(p).to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Uint8Array".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "{ lo: number; hi: number }".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Raw ABI return type for peer caller methods — must match the `result` shape
/// that `emit_ts_guest_host_contract_out_setup` initialises in the generated body.
/// Peer callers return raw ABI values; the caller is responsible for decoding.
fn ts_peer_raw_return_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            "{ ptr_lo: number; ptr_hi: number; len: number }".to_owned()
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            "{ ptr_lo: number; ptr_hi: number; len: number; cap: number }".to_owned()
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "{ lo: number; hi: number }".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::Primitive(p) => {
            if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                "{ lo: number; hi: number }".to_owned()
            } else {
                "number".to_owned()
            }
        }
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// TypeScript primitive type for guest callers (ergonomic, not ABI-level).
fn ts_primitive_guest(p: &PrimitiveType) -> &'static str {
    match p {
        PrimitiveType::U8
        | PrimitiveType::U16
        | PrimitiveType::U32
        | PrimitiveType::I8
        | PrimitiveType::I16
        | PrimitiveType::I32
        | PrimitiveType::F32
        | PrimitiveType::F64 => "number",
        PrimitiveType::Bool => "boolean",
        PrimitiveType::U64 | PrimitiveType::I64 => "{ lo: number; hi: number }",
    }
}

/// Generate one guest-side host contract caller class.
fn generate_ts_guest_host_contract_caller(
    out: &mut String,
    contract: &ResolvedHostContract,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let class_name: String = host_contract_name_to_ts_caller(&contract.name);
    let contract_id_lo: u32 = (contract.contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract.contract_id >> 32) as u32;

    emit_js_attributes(out, CustomizableNode::HostContract, &contract.langs, "");
    if let Some(docs) = contract.docs.as_deref() {
        let caller_docs: String = format!(
            "Guest caller for host contract `{}` (id=0x{:016X})\n\n{docs}",
            contract.name, contract.contract_id
        );
        write_jsdoc(out, "", Some(&caller_docs), &[], None);
    } else {
        out.push_str(&format!(
            "/**\n * Guest caller for host contract `{}` (id=0x{:016X})\n */\n",
            contract.name, contract.contract_id
        ));
    }
    out.push_str(&format!("export class {} {{\n", class_name));
    out.push_str("    private _minVersion: number;\n");
    // The bridge and host pointer are threaded in explicitly (no global — Rule 12)
    // and stored as instance state so every method reaches the host through them.
    out.push_str("    private _bridge: any;\n");
    out.push_str("    private _hostPtr: { lo: number; hi: number };\n\n");

    out.push_str(
        "    private constructor(bridge: any, hostPtr: { lo: number; hi: number }, minVersion: number) {\n",
    );
    out.push_str("        this._bridge = bridge;\n");
    out.push_str("        this._hostPtr = hostPtr;\n");
    out.push_str("        this._minVersion = minVersion;\n");
    out.push_str("    }\n\n");

    // langprint renders each method's FORM (indent-1 class member); bodies are verbatim.
    let mut from_host_body: String = String::new();
    from_host_body.push_str("        if (!bridge || !bridge.callHostContract) {\n");
    from_host_body.push_str("            return null;\n");
    from_host_body.push_str("        }\n");
    from_host_body.push_str(&format!(
        "        return new {}(bridge, hostPtr, minVersion);",
        class_name
    ));
    out.push_str(&render_js_method(
        "fromHost",
        true,
        vec![
            JsParameter {
                name: "bridge".to_owned(),
                default: None,
                type_doc: Some("any".to_owned()),
            },
            JsParameter {
                name: "hostPtr".to_owned(),
                default: None,
                type_doc: Some("{ lo: number; hi: number }".to_owned()),
            },
            JsParameter {
                name: "minVersion".to_owned(),
                default: Some("0".to_owned()),
                type_doc: Some("number".to_owned()),
            },
        ],
        Some(format!("{class_name} | null")),
        "/** Factory method - creates caller instance or null if the bridge is unavailable. */",
        from_host_body,
    )?);
    out.push('\n');

    out.push_str(&render_js_method(
        "isValid",
        false,
        Vec::new(),
        Some("boolean".to_owned()),
        "/** Check if the bridge is available. */",
        "        return !!(this._bridge && this._bridge.callHostContract);".to_owned(),
    )?);
    out.push('\n');

    for func in &contract.functions {
        generate_ts_guest_host_contract_method(out, func, contract_id_lo, contract_id_hi, ir)?;
    }

    out.push_str("}\n\n");
    Ok(())
}

/// Generate one method for a guest-side host contract caller.
fn generate_ts_guest_host_contract_method(
    dst: &mut String,
    func: &ResolvedFunction,
    contract_id_lo: u32,
    contract_id_hi: u32,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let fn_id: u32 = func.function_id;
    // Returns are RAW ABI values — declared type must match the shape that
    // emit_ts_guest_host_contract_readback produces (e.g. StringView →
    // {ptr_lo,ptr_hi,len}), not the ergonomic string/Uint8Array.
    let return_type: String = match &func.returns {
        Some(ty) => ts_peer_raw_return_type(ty),
        None => "void".to_owned(),
    };
    let has_return: bool = func.returns.is_some();

    // langprint renders the method FORM (indent-1 class member); the body is verbatim.
    let parameters: Vec<JsParameter> = func
        .params
        .iter()
        .map(|p: &ResolvedParam| JsParameter {
            name: p.name.clone(),
            default: None,
            type_doc: Some(ts_guest_caller_param_type(&p.ty)),
        })
        .collect::<Vec<JsParameter>>();
    let mut jsdoc: String = String::new();
    emit_js_function_attributes(&mut jsdoc, func, "");
    let documented_params: Vec<(&str, Option<&str>)> = func
        .params
        .iter()
        .map(|param| (param.name.as_str(), param.docs.as_deref()))
        .collect();
    if func.docs.is_some()
        || func.params.iter().any(|param| param.docs.is_some())
        || func.return_docs.is_some()
    {
        write_jsdoc(
            &mut jsdoc,
            "",
            func.docs.as_deref(),
            &documented_params,
            func.return_docs.as_deref(),
        );
    } else {
        jsdoc.push_str(&format!("/** Call `{}` */", func.name));
    }

    // The body is accumulated into `body` (aliased as `out` so the existing
    // push_str lines below are unchanged), then rendered as the verbatim slot.
    let mut body: String = String::new();
    let out: &mut String = &mut body;

    out.push_str("        const polyplug = this._bridge;\n");
    out.push_str("        if (!polyplug || !polyplug.callHostContract) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");
    emit_ts_caller_alloc_shim(out);
    out.push_str("        try {\n");

    emit_ts_guest_host_contract_args_setup(out, func, ir)?;
    emit_ts_guest_host_contract_out_setup(out, &func.returns, ir)?;

    out.push_str(&format!(
        "        const errCode: number = polyplug.callHostContract(0x{:08X}, 0x{:08X}, this._minVersion, {}, argsPtr, outPtr);\n",
        contract_id_lo, contract_id_hi, fn_id
    ));
    // Throw with the ABI error code (JS host SDK convention) instead of
    // silently returning null and discarding the code.
    out.push_str("        if (errCode !== 0) {\n");
    out.push_str(&format!(
        "            throw new Error(`host contract call {} failed (code ${{errCode}})`);\n",
        func.name
    ));
    out.push_str("        }\n");

    if has_return {
        emit_ts_guest_host_contract_readback(out, func.returns.as_ref(), ir)?;
        out.push_str("        return result;\n");
    }

    emit_ts_caller_free_shim(out);
    if body.ends_with('\n') {
        body.pop();
    }
    dst.push_str(&render_js_method(
        &func.name,
        false,
        parameters,
        Some(return_type),
        &jsdoc,
        body,
    )?);
    dst.push('\n');
    Ok(())
}

/// Emit the caller-local host allocator + free-list used for a guest→host /
/// peer-caller method's transient arg/out buffers.
///
/// The caller runs inside a guest dispatch but cannot reach that dispatch's
/// per-call arena (its method signature is author-defined), so — mirroring the
/// lua/python callers, which use FFI stack memory — JS allocates these transient
/// buffers from the HOST allocator and frees them explicitly when the method
/// returns. `_callerAlloc(size)` returns the same `[lo, hi]` pair `arenaAlloc`
/// did, so the offset-writing emitters are unchanged; it records `[lo, hi, size]`
/// on `_frees` so `emit_ts_caller_free_shim`'s `finally` releases every region
/// even if the call throws. A returned StringView/Buffer points at HOST-owned
/// memory (not these buffers), so freeing them after read-back is sound.
fn emit_ts_caller_alloc_shim(out: &mut String) {
    out.push_str("        const _frees: number[][] = [];\n");
    out.push_str(
        "        const _callerAlloc = (sz: number): number[] => { const _a = polyplug.alloc(sz); _frees.push([_a[0], _a[1], sz]); return _a; };\n",
    );
}

/// Emit the `finally` block that frees every region recorded by
/// `_callerAlloc`. Closes the `try` opened by `emit_ts_caller_alloc_shim`.
fn emit_ts_caller_free_shim(out: &mut String) {
    out.push_str("        } finally {\n");
    out.push_str(
        "            for (const _f of _frees) { polyplug.free(_f[0], _f[1], _f[2], 1); }\n",
    );
    out.push_str("        }\n");
}

/// `(size, align)` of one caller-pack slot for the guest→host / peer caller
/// args pack. User-defined ENUM types use their repr width; user-defined
/// non-enum (struct-by-value) params use their full C-layout size/alignment
/// (computed by `js_c_size` / `js_c_align`) and are marshalled field-by-field
/// by `emit_ts_caller_pack_value`, matching the guest-side wrappers and every
/// other host caller.
fn js_caller_slot_layout(
    ty: &ResolvedTypeRef,
    ir: &ValidatedIr,
) -> Result<(usize, usize), PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8 | PrimitiveType::I8 | PrimitiveType::Bool => (1, 1),
            PrimitiveType::U16 | PrimitiveType::I16 => (2, 2),
            PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => (4, 4),
            PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => (8, 8),
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok((16, 8)),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok((24, 8)),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok((8, 8)),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok((0, 1)),
        ResolvedTypeRef::UserDefined(_) => match js_enum_for_type(ty, &ir.enums) {
            Some(e) => {
                let size: usize = js_repr_size(&e.repr);
                Ok((size, size))
            }
            None => Ok((js_c_size(ty, ir)?, js_c_align(ty, ir)?)),
        },
    }
}

/// Round `offset` up to the next multiple of `align` (C struct layout).
fn align_up(offset: usize, align: usize) -> usize {
    if align == 0 {
        return offset;
    }
    offset.div_ceil(align) * align
}

/// Byte width of an enum's declared repr.
fn js_repr_size(repr: &ReprType) -> usize {
    match repr {
        ReprType::U8 => 1,
        ReprType::U16 => 2,
        ReprType::U32 => 4,
        ReprType::U64 => 8,
    }
}

/// Find the enum definition backing a user-defined type, if it is an enum.
fn js_enum_for_type<'a>(ty: &ResolvedTypeRef, enums: &'a [EnumDef]) -> Option<&'a EnumDef> {
    match ty {
        ResolvedTypeRef::UserDefined(name) => enums.iter().find(|e: &&EnumDef| &e.name == name),
        _ => None,
    }
}

/// Find the struct definition backing a user-defined type, if it is a struct.
fn js_struct_for_type<'a>(
    ty: &ResolvedTypeRef,
    types: &'a [ResolvedType],
) -> Option<&'a ResolvedType> {
    match ty {
        ResolvedTypeRef::UserDefined(name) => {
            types.iter().find(|t: &&ResolvedType| &t.name == name)
        }
        _ => None,
    }
}

/// C-layout alignment of one ABI value as packed by every host-side caller
/// (`#[repr(C)]` / ctypes / LuaJIT cdef / LayoutKind.Sequential all use
/// natural alignment).
fn js_c_align(ty: &ResolvedTypeRef, ir: &ValidatedIr) -> Result<usize, PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8 | PrimitiveType::I8 | PrimitiveType::Bool => 1,
            PrimitiveType::U16 | PrimitiveType::I16 => 2,
            PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
            PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => 8,
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView)
        | ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)
        | ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok(8),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(1),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                return Ok(js_repr_size(&e.repr));
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut max_align: usize = 1;
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    if a > max_align {
                        max_align = a;
                    }
                }
                return Ok(max_align);
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// C-layout size of one ABI value as packed by every host-side caller.
fn js_c_size(ty: &ResolvedTypeRef, ir: &ValidatedIr) -> Result<usize, PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8 | PrimitiveType::I8 | PrimitiveType::Bool => 1,
            PrimitiveType::U16 | PrimitiveType::I16 => 2,
            PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
            PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => 8,
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok(16),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok(24),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok(8),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(0),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                return Ok(js_repr_size(&e.repr));
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut offset: usize = 0;
                let mut max_align: usize = 1;
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    if a > max_align {
                        max_align = a;
                    }
                    offset = align_up(offset, a);
                    offset += js_c_size(&field.ty, ir)?;
                }
                return Ok(align_up(offset, max_align));
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Render `base + off` as a JS pointer expression (omits `+ 0`).
fn js_ptr_at(base: &str, off: usize) -> String {
    if off == 0 {
        base.to_owned()
    } else {
        format!("{base} + {off}")
    }
}

/// JS expression that reads one ABI value of `ty` at `base + off` through the
/// loader bridge (readByte/readI32/readU32/readF32/readF64). Enums read their
/// repr-width raw integer; user structs read field-by-field into a (possibly
/// nested) object literal to unbounded depth. Narrow signed integers are
/// sign-extended with shift pairs because the bridge only exposes byte and
/// 32-bit reads.
fn js_read_expr(
    ty: &ResolvedTypeRef,
    base: &str,
    off: usize,
    ir: &ValidatedIr,
) -> Result<String, PolyplugcError> {
    let p: String = js_ptr_at(base, off);
    match ty {
        ResolvedTypeRef::Primitive(prim) => Ok(match prim {
            PrimitiveType::U8 => format!("polyplug.readByte({p})"),
            PrimitiveType::I8 => format!("((polyplug.readByte({p}) << 24) >> 24)"),
            PrimitiveType::U16 => format!(
                "(polyplug.readByte({p}) | (polyplug.readByte({p1}) << 8))",
                p1 = js_ptr_at(base, off + 1)
            ),
            PrimitiveType::I16 => format!(
                "(((polyplug.readByte({p}) | (polyplug.readByte({p1}) << 8)) << 16) >> 16)",
                p1 = js_ptr_at(base, off + 1)
            ),
            PrimitiveType::U32 => format!("polyplug.readU32({p})"),
            PrimitiveType::I32 => format!("polyplug.readI32({p})"),
            PrimitiveType::F32 => format!("polyplug.readF32({p})"),
            PrimitiveType::F64 => format!("polyplug.readF64({p})"),
            PrimitiveType::Bool => format!("(polyplug.readByte({p}) !== 0)"),
            PrimitiveType::U64 | PrimitiveType::I64 => format!(
                "{{ lo: polyplug.readU32({p}), hi: polyplug.readU32({p4}) }}",
                p4 = js_ptr_at(base, off + 4)
            ),
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok(format!(
            "{{ lo: polyplug.readU32({p}), hi: polyplug.readU32({p4}) }}",
            p4 = js_ptr_at(base, off + 4)
        )),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok(format!(
            "{{ ptr_lo: polyplug.readU32({p}), ptr_hi: polyplug.readU32({p4}), len: polyplug.readU32({p8}) }}",
            p4 = js_ptr_at(base, off + 4),
            p8 = js_ptr_at(base, off + 8)
        )),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok(format!(
            "{{ ptr_lo: polyplug.readU32({p}), ptr_hi: polyplug.readU32({p4}), len: polyplug.readU32({p8}), cap: polyplug.readU32({p16}) }}",
            p4 = js_ptr_at(base, off + 4),
            p8 = js_ptr_at(base, off + 8),
            p16 = js_ptr_at(base, off + 16)
        )),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Err(PolyplugcError::UnsupportedType {
            type_name: "void".to_owned(),
            lang: "js-quickjs".to_owned(),
        }),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                return Ok(match e.repr {
                    ReprType::U8 => format!("polyplug.readByte({p})"),
                    ReprType::U16 => format!(
                        "(polyplug.readByte({p}) | (polyplug.readByte({p1}) << 8))",
                        p1 = js_ptr_at(base, off + 1)
                    ),
                    ReprType::U32 => format!("polyplug.readU32({p})"),
                    // Exact for values < 2^53 (QuickJS numbers are f64; the
                    // generator's lo/hi convention applies to declared u64
                    // PRIMITIVES, while enum VALUES are plain numbers).
                    ReprType::U64 => format!(
                        "(polyplug.readU32({p}) + polyplug.readU32({p4}) * 4294967296)",
                        p4 = js_ptr_at(base, off + 4)
                    ),
                });
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut offset: usize = off;
                let mut fields: Vec<String> = Vec::new();
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    offset = align_up(offset, a);
                    let expr: String = js_read_expr(&field.ty, base, offset, ir)?;
                    fields.push(format!("{}: {}", field.name, expr));
                    offset += js_c_size(&field.ty, ir)?;
                }
                return Ok(format!("{{ {} }}", fields.join(", ")));
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Emit the dispatch wrapper's return write into `out_ptr` (offset 0).
///
/// For a top-level `StringView` return the author returns a plain JS string and
/// the GENERATED wrapper arena-allocates it via `bridge.arenaAlloc(size,
/// arena_ptr)` using the per-call arena pointer the loader threads in (no per-VM
/// global, no author-side arena — mirrors the lua/python reference). Every other
/// return shape (scalars, Buffer, struct, enum) is written by `emit_js_write_value`
/// from the value the author returns directly.
fn emit_js_guest_return_write(
    out: &mut String,
    ret_ty: &ResolvedTypeRef,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    if matches!(ret_ty, ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) {
        // The author returns a plain string; encode it, arena-allocate the bytes
        // from THIS call's arena, and write the StringView into the out slot.
        out.push_str("    const _retBytes = _ppEncodeUtf8(result);\n");
        out.push_str(
            "    const _retBuf = polyplug.arenaAlloc(_retBytes.length > 0 ? _retBytes.length : 1, arena_ptr);\n",
        );
        out.push_str("    const _retPtr = _retBuf[0] + _retBuf[1] * 4294967296;\n");
        out.push_str(
            "    for (let _i = 0; _i < _retBytes.length; _i++) { polyplug.writeByte(_retPtr + _i, _retBytes[_i]); }\n",
        );
        out.push_str("    polyplug.writeU32(out_ptr, _retBuf[0]);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 4, _retBuf[1]);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 8, _retBytes.length);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 12, 0);\n");
        return Ok(());
    }
    let mut ctx: JsGuestMarshal<'_> = JsGuestMarshal { ir, uid: 0 };
    emit_js_guest_marshal(out, "    ", ret_ty, "out_ptr", 0, "result", &mut ctx)
}

/// Threaded state for the recursive guest-return marshaler: the IR (for struct/
/// enum/layout lookups) and a monotonic counter that names temporaries uniquely
/// across nesting levels.
struct JsGuestMarshal<'a> {
    ir: &'a ValidatedIr,
    uid: usize,
}

/// Element `ResolvedTypeRef` for an array-wrapper's element name (the suffix of
/// `ArrayOf_<element>`): a primitive, an ABI builtin, or a user struct/enum.
fn element_type_ref(name: &str) -> ResolvedTypeRef {
    if let Some(p) = PrimitiveType::parse(name) {
        ResolvedTypeRef::Primitive(p)
    } else if let Some(b) = AbiBuiltin::parse(name) {
        ResolvedTypeRef::AbiType(b)
    } else {
        ResolvedTypeRef::UserDefined(name.to_owned())
    }
}

/// Marshal a guest RETURN value `value` (of `ty`) into the out buffer at
/// `base + off`, ALLOCATING variable-size parts from the per-call arena
/// (`polyplug.arenaAlloc(size, arena_ptr)`). Unlike `emit_js_write_value` (which
/// assumes `StringView`/array pointers are already set), this is the return path
/// where the author hands back ergonomic JS values: a string for `StringView`, an
/// object for a struct, an array of objects for `ArrayOf_T`.
fn emit_js_guest_marshal(
    out: &mut String,
    indent: &str,
    ty: &ResolvedTypeRef,
    base: &str,
    off: usize,
    value: &str,
    ctx: &mut JsGuestMarshal<'_>,
) -> Result<(), PolyplugcError> {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            emit_js_guest_string_view(out, indent, base, off, value, ctx);
            Ok(())
        }
        ResolvedTypeRef::UserDefined(name) if array_element_name(name).is_some() => {
            let element: &str = array_element_name(name).unwrap_or(name);
            emit_js_guest_marshal_array(out, indent, element, base, off, value, ctx)
        }
        // A struct (not an enum, which is a scalar) recurses field-by-field so its
        // embedded StringView / array fields are allocated too.
        ResolvedTypeRef::UserDefined(_)
            if js_enum_for_type(ty, &ctx.ir.enums).is_none()
                && js_struct_for_type(ty, &ctx.ir.types).is_some() =>
        {
            let s: &ResolvedType =
                js_struct_for_type(ty, &ctx.ir.types).unwrap_or_else(|| unreachable!());
            let mut offset: usize = off;
            for field in &s.fields {
                let a: usize = js_c_align(&field.ty, ctx.ir)?;
                offset = align_up(offset, a);
                let field_value: String = format!("{value}.{}", field.name);
                emit_js_guest_marshal(out, indent, &field.ty, base, offset, &field_value, ctx)?;
                offset += js_c_size(&field.ty, ctx.ir)?;
            }
            Ok(())
        }
        // Scalars, enums, Ptr, Buffer, Void: fixed-size, written directly.
        _ => emit_js_write_value(out, indent, ty, base, off, value, ctx.ir),
    }
}

/// Emit a `StringView` return write for a guest return. Author-facing top-level
/// strings are encoded into the call arena; nested fields may already carry their
/// canonical `{ ptr_lo, ptr_hi, len }` representation from an imported domain
/// package and are copied without re-encoding.
fn emit_js_guest_string_view(
    out: &mut String,
    indent: &str,
    base: &str,
    off: usize,
    value: &str,
    ctx: &mut JsGuestMarshal<'_>,
) {
    let id: usize = ctx.uid;
    ctx.uid += 1;
    out.push_str(&format!("{indent}if (typeof {value} === \"string\") {{\n"));
    out.push_str(&format!(
        "{indent}    const _sb{id} = _ppEncodeUtf8({value});\n"
    ));
    out.push_str(&format!(
        "{indent}    const _sbuf{id} = polyplug.arenaAlloc(_sb{id}.length > 0 ? _sb{id}.length : 1, arena_ptr);\n"
    ));
    out.push_str(&format!(
        "{indent}    const _sp{id} = _sbuf{id}[0] + _sbuf{id}[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "{indent}    for (let _i{id} = 0; _i{id} < _sb{id}.length; _i{id}++) {{ polyplug.writeByte(_sp{id} + _i{id}, _sb{id}[_i{id}]); }}\n"
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, _sbuf{id}[0]);\n",
        js_ptr_at(base, off)
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, _sbuf{id}[1]);\n",
        js_ptr_at(base, off + 4)
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, _sb{id}.length);\n",
        js_ptr_at(base, off + 8)
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, 0);\n",
        js_ptr_at(base, off + 12)
    ));
    out.push_str(&format!("{indent}}} else {{\n"));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, {value}.ptr_lo);\n",
        js_ptr_at(base, off)
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, {value}.ptr_hi);\n",
        js_ptr_at(base, off + 4)
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, {value}.len);\n",
        js_ptr_at(base, off + 8)
    ));
    out.push_str(&format!(
        "{indent}    polyplug.writeU32({}, 0);\n",
        js_ptr_at(base, off + 12)
    ));
    out.push_str(&format!("{indent}}}\n"));
}

/// Emit an allocating array write: allocate `value.length` elements from the
/// arena (align-1 allocator, so over-allocate and round the base up to the
/// element alignment), marshal each element, then write `items`/`len` at
/// `base + off`.
fn emit_js_guest_marshal_array(
    out: &mut String,
    indent: &str,
    element: &str,
    base: &str,
    off: usize,
    value: &str,
    ctx: &mut JsGuestMarshal<'_>,
) -> Result<(), PolyplugcError> {
    let id: usize = ctx.uid;
    ctx.uid += 1;
    let elem_ref: ResolvedTypeRef = element_type_ref(element);
    let esize: usize = js_c_size(&elem_ref, ctx.ir)?;
    let ealign: usize = js_c_align(&elem_ref, ctx.ir)?;
    let write_items_len =
        |out: &mut String, ind: &str, items_lo: &str, items_hi: &str, len: &str| {
            out.push_str(&format!(
                "{ind}polyplug.writeU32({}, {items_lo});\n",
                js_ptr_at(base, off)
            ));
            out.push_str(&format!(
                "{ind}polyplug.writeU32({}, {items_hi});\n",
                js_ptr_at(base, off + 4)
            ));
            out.push_str(&format!(
                "{ind}polyplug.writeU32({}, {len});\n",
                js_ptr_at(base, off + 8)
            ));
            out.push_str(&format!(
                "{ind}polyplug.writeU32({}, 0);\n",
                js_ptr_at(base, off + 12)
            ));
        };
    out.push_str(&format!("{indent}const _n{id} = {value}.length;\n"));
    out.push_str(&format!("{indent}if (_n{id} === 0) {{\n"));
    write_items_len(out, &format!("{indent}    "), "0", "0", "0");
    out.push_str(&format!("{indent}}} else {{\n"));
    let inner: String = format!("{indent}    ");
    out.push_str(&format!(
        "{inner}const _rb{id} = polyplug.arenaAlloc(_n{id} * {esize} + {}, arena_ptr);\n",
        ealign - 1
    ));
    out.push_str(&format!(
        "{inner}const _raddr{id} = _rb{id}[0] + _rb{id}[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "{inner}const _bs{id} = Math.ceil(_raddr{id} / {ealign}) * {ealign};\n"
    ));
    out.push_str(&format!(
        "{inner}for (let _ix{id} = 0; _ix{id} < _n{id}; _ix{id}++) {{\n"
    ));
    out.push_str(&format!("{inner}    const _el{id} = {value}[_ix{id}];\n"));
    out.push_str(&format!(
        "{inner}    const _ep{id} = _bs{id} + _ix{id} * {esize};\n"
    ));
    emit_js_guest_marshal(
        out,
        &format!("{inner}    "),
        &elem_ref,
        &format!("_ep{id}"),
        0,
        &format!("_el{id}"),
        ctx,
    )?;
    out.push_str(&format!("{inner}}}\n"));
    write_items_len(
        out,
        &inner,
        &format!("_bs{id} % 4294967296"),
        &format!("Math.floor(_bs{id} / 4294967296)"),
        &format!("_n{id}"),
    );
    out.push_str(&format!("{indent}}}\n"));
    Ok(())
}

/// Emit statements writing `value` (a JS expression of `ty`'s shape) into the
/// out slot at `base + off` through the loader bridge. The mirror of
/// `js_read_expr` — same C layout, same lo/hi and {ptr_lo,ptr_hi,len} shapes.
fn emit_js_write_value(
    out: &mut String,
    indent: &str,
    ty: &ResolvedTypeRef,
    base: &str,
    off: usize,
    value: &str,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let p: String = js_ptr_at(base, off);
    match ty {
        ResolvedTypeRef::Primitive(prim) => {
            match prim {
                PrimitiveType::U8 | PrimitiveType::I8 => {
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p}, {value} & 0xFF);\n"
                    ));
                }
                PrimitiveType::U16 | PrimitiveType::I16 => {
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p}, {value} & 0xFF);\n"
                    ));
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p1}, ({value} >> 8) & 0xFF);\n",
                        p1 = js_ptr_at(base, off + 1)
                    ));
                }
                PrimitiveType::U32 => {
                    out.push_str(&format!("{indent}polyplug.writeU32({p}, {value});\n"));
                }
                PrimitiveType::I32 => {
                    out.push_str(&format!("{indent}polyplug.writeI32({p}, {value});\n"));
                }
                PrimitiveType::F32 => {
                    out.push_str(&format!("{indent}polyplug.writeF32({p}, {value});\n"));
                }
                PrimitiveType::F64 => {
                    out.push_str(&format!("{indent}polyplug.writeF64({p}, {value});\n"));
                }
                PrimitiveType::Bool => {
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p}, {value} ? 1 : 0);\n"
                    ));
                }
                PrimitiveType::U64 | PrimitiveType::I64 => {
                    out.push_str(&format!("{indent}polyplug.writeU32({p}, {value}.lo);\n"));
                    out.push_str(&format!(
                        "{indent}polyplug.writeU32({p4}, {value}.hi);\n",
                        p4 = js_ptr_at(base, off + 4)
                    ));
                }
            }
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(&format!("{indent}polyplug.writeU32({p}, {value}.lo);\n"));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p4}, {value}.hi);\n",
                p4 = js_ptr_at(base, off + 4)
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p}, {value}.ptr_lo);\n"
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p4}, {value}.ptr_hi);\n",
                p4 = js_ptr_at(base, off + 4)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p8}, {value}.len);\n",
                p8 = js_ptr_at(base, off + 8)
            ));
            // High half of the usize len: zero (lengths are < 2^32).
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p12}, 0);\n",
                p12 = js_ptr_at(base, off + 12)
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p}, {value}.ptr_lo);\n"
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p4}, {value}.ptr_hi);\n",
                p4 = js_ptr_at(base, off + 4)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p8}, {value}.len);\n",
                p8 = js_ptr_at(base, off + 8)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p12}, 0);\n",
                p12 = js_ptr_at(base, off + 12)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p16}, {value}.cap);\n",
                p16 = js_ptr_at(base, off + 16)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p20}, 0);\n",
                p20 = js_ptr_at(base, off + 20)
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(()),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                match e.repr {
                    ReprType::U8 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeByte({p}, Number({value}) & 0xFF);\n"
                        ));
                    }
                    ReprType::U16 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeByte({p}, Number({value}) & 0xFF);\n"
                        ));
                        out.push_str(&format!(
                            "{indent}polyplug.writeByte({p1}, (Number({value}) >> 8) & 0xFF);\n",
                            p1 = js_ptr_at(base, off + 1)
                        ));
                    }
                    ReprType::U32 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeU32({p}, Number({value}));\n"
                        ));
                    }
                    ReprType::U64 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeU32({p}, Number({value}) >>> 0);\n"
                        ));
                        out.push_str(&format!(
                            "{indent}polyplug.writeU32({p4}, Math.floor(Number({value}) / 4294967296));\n",
                            p4 = js_ptr_at(base, off + 4)
                        ));
                    }
                }
                return Ok(());
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut offset: usize = off;
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    offset = align_up(offset, a);
                    let field_value: String = format!("{value}.{}", field.name);
                    emit_js_write_value(out, indent, &field.ty, base, offset, &field_value, ir)?;
                    offset += js_c_size(&field.ty, ir)?;
                }
                return Ok(());
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Emit the file-level `_ppEncodeUtf8(str)` helper.
///
/// The QuickJS loader does NOT provide `TextEncoder` (it is absent in QuickJS —
/// see `sdks/js/guest/polyplug_guest.js`), so generated guest code must never
/// call `new TextEncoder()` unconditionally. This emits a manual UTF-8 encoder
/// (mirroring the SDK's `_encodeUtf8`) guarded by a `typeof TextEncoder` check,
/// so emitted marshalling works in both QuickJS and TextEncoder-bearing runtimes.
fn emit_ts_utf8_encoder_helper(out: &mut String) -> Result<(), PolyplugcError> {
    // langprint renders the `function _ppEncodeUtf8(str: string): Uint8Array` FORM;
    // the encoder body (nested loop + branch indentation) is the verbatim slot.
    out.push_str("// UTF-8 encoder usable in QuickJS (where TextEncoder is absent).\n");
    let mut body: String = String::new();
    body.push_str(
        "    if (typeof TextEncoder !== 'undefined') { return new TextEncoder().encode(str); }\n",
    );
    body.push_str("    const out: number[] = [];\n");
    body.push_str("    for (let i = 0; i < str.length; i++) {\n");
    body.push_str("        let code = str.charCodeAt(i);\n");
    body.push_str("        if (code >= 0xD800 && code <= 0xDBFF) {\n");
    body.push_str("            const low = str.charCodeAt(++i);\n");
    body.push_str("            code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);\n");
    body.push_str("        }\n");
    body.push_str("        if (code < 0x80) { out.push(code); }\n");
    body.push_str(
        "        else if (code < 0x800) { out.push(0xC0 | (code >> 6), 0x80 | (code & 0x3F)); }\n",
    );
    body.push_str("        else if (code < 0x10000) { out.push(0xE0 | (code >> 12), 0x80 | ((code >> 6) & 0x3F), 0x80 | (code & 0x3F)); }\n");
    body.push_str("        else { out.push(0xF0 | (code >> 18), 0x80 | ((code >> 12) & 0x3F), 0x80 | ((code >> 6) & 0x3F), 0x80 | (code & 0x3F)); }\n");
    body.push_str("    }\n");
    body.push_str("    return new Uint8Array(out);");
    out.push_str(&render_js_defn_fn(
        "_ppEncodeUtf8",
        js_params(&[("str", "string")]),
        Some("Uint8Array".to_owned()),
        body,
        false,
    )?);
    out.push('\n');
    Ok(())
}

/// Emit a StringView into `buf` at `offset` using `_callerAlloc` + writeU32/writeByte.
/// `value` is the JS string EXPRESSION to encode (e.g. `name` or `o.inner.s`);
/// `local` is a valid identifier base for the temporaries (e.g. `name` or `sv0`),
/// kept separate so nested struct fields (whose value expression contains `.`)
/// can still name their temporaries.
fn emit_ts_write_string_view(
    out: &mut String,
    value: &str,
    local: &str,
    args_ptr: &str,
    offset: usize,
) {
    let n: &str = local;
    let ap: &str = args_ptr;
    out.push_str(&format!(
        "        const _{n}Bytes = _ppEncodeUtf8({value});\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataBuf = _callerAlloc(_{n}Bytes.length > 0 ? _{n}Bytes.length : 1);\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataPtr = _{n}DataBuf[0] + _{n}DataBuf[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "        for (let _i = 0; _i < _{n}Bytes.length; _i++) {{ polyplug.writeByte(_{n}DataPtr + _i, _{n}Bytes[_i]); }}\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {offset}, _{n}DataBuf[0]);\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}DataBuf[1]);\n",
        offset + 4
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}Bytes.length);\n",
        offset + 8
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 12
    ));
}

/// Emit a Buffer (Uint8Array) into `buf` at `offset` using `_callerAlloc` + writeU32/writeByte.
/// `value` is the JS Uint8Array EXPRESSION; `local` is the identifier base for the
/// temporaries (see `emit_ts_write_string_view`).
fn emit_ts_write_buffer(out: &mut String, value: &str, local: &str, args_ptr: &str, offset: usize) {
    let n: &str = local;
    let ap: &str = args_ptr;
    out.push_str(&format!(
        "        const _{n}DataBuf = _callerAlloc({value}.length > 0 ? {value}.length : 1);\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataPtr = _{n}DataBuf[0] + _{n}DataBuf[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "        for (let _i = 0; _i < {value}.length; _i++) {{ polyplug.writeByte(_{n}DataPtr + _i, {value}[_i]); }}\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {offset}, _{n}DataBuf[0]);\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}DataBuf[1]);\n",
        offset + 4
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, {value}.length);\n",
        offset + 8
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 12
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, {value}.length);\n",
        offset + 16
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 20
    ));
}

/// Recursively pack `value` (a JS expression of `ty`'s ergonomic shape) into the
/// caller args buffer at `{args_ptr} + {offset}` through the QuickJS bridge.
///
/// StringView/Buffer leaves are arena-allocated from their string/Uint8Array;
/// struct fields recurse field-by-field at their C-layout offsets to unbounded
/// depth; every other leaf (primitive, Ptr, enum) delegates to
/// `emit_js_write_value`. This mirrors the Deno caller's `emit_deno_write_value`,
/// so the JS guest caller marshals struct-by-value params with no type-support
/// limitation. `sv_idx` yields unique identifier bases for the arena temporaries.
fn emit_ts_caller_pack_value(
    out: &mut String,
    ty: &ResolvedTypeRef,
    args_ptr: &str,
    offset: usize,
    value: &str,
    ir: &ValidatedIr,
    sv_idx: &mut u32,
) -> Result<(), PolyplugcError> {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            let local: String = format!("sv{}", *sv_idx);
            *sv_idx += 1;
            emit_ts_write_string_view(out, value, &local, args_ptr, offset);
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            let local: String = format!("buf{}", *sv_idx);
            *sv_idx += 1;
            emit_ts_write_buffer(out, value, &local, args_ptr, offset);
            Ok(())
        }
        ResolvedTypeRef::UserDefined(_) if js_struct_for_type(ty, &ir.types).is_some() => {
            let s: &ResolvedType = js_struct_for_type(ty, &ir.types).ok_or_else(|| {
                PolyplugcError::UnsupportedType {
                    type_name: "struct".to_owned(),
                    lang: "js-quickjs".to_owned(),
                }
            })?;
            let mut field_off: usize = offset;
            for field in &s.fields {
                let align: usize = js_c_align(&field.ty, ir)?;
                field_off = align_up(field_off, align);
                let field_value: String = format!("{value}.{}", field.name);
                emit_ts_caller_pack_value(
                    out,
                    &field.ty,
                    args_ptr,
                    field_off,
                    &field_value,
                    ir,
                    sv_idx,
                )?;
                field_off += js_c_size(&field.ty, ir)?;
            }
            Ok(())
        }
        _ => emit_js_write_value(out, "        ", ty, args_ptr, offset, value, ir),
    }
}

/// Emit the argsPtr setup for a TypeScript guest host contract / peer method.
///
/// Allocates every transient buffer through `_callerAlloc` (a host-allocator
/// shim that records each region on the method's `_frees` list); the method's
/// `finally` block frees them all. The caller cannot reach the dispatch's
/// per-call arena, so it uses host alloc+free here, mirroring the lua/python
/// callers' FFI-stack buffers (Rule 12 — no per-call global arena read).
fn emit_ts_guest_host_contract_args_setup(
    out: &mut String,
    func: &ResolvedFunction,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let enums: &[EnumDef] = &ir.enums;
    if func.params.is_empty() {
        out.push_str("        const argsPtr = 0;\n");
        return Ok(());
    }

    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "        const _{0}Bytes = _ppEncodeUtf8({0});\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataBuf = _callerAlloc(_{0}Bytes.length > 0 ? _{0}Bytes.length : 1);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataPtr = _{0}DataBuf[0] + _{0}DataBuf[1] * 4294967296;\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        for (let _i = 0; _i < _{0}Bytes.length; _i++) {{ polyplug.writeByte(_{0}DataPtr + _i, _{0}Bytes[_i]); }}\n",
                    param.name
                ));
                out.push_str("        const _argsBuf = _callerAlloc(16);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, _{0}DataBuf[0]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 4, _{0}DataBuf[1]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 8, _{0}Bytes.length);\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 12, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "        const _{0}DataBuf = _callerAlloc({0}.length > 0 ? {0}.length : 1);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataPtr = _{0}DataBuf[0] + _{0}DataBuf[1] * 4294967296;\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        for (let _i = 0; _i < {0}.length; _i++) {{ polyplug.writeByte(_{0}DataPtr + _i, {0}[_i]); }}\n",
                    param.name
                ));
                out.push_str("        const _argsBuf = _callerAlloc(24);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, _{0}DataBuf[0]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 4, _{0}DataBuf[1]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 8, {0}.length);\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 12, 0);\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 16, {0}.length);\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 20, 0);\n");
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr, {}.lo);\n",
                        param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr + 4, {}.hi);\n",
                        param.name
                    ));
                } else if matches!(p, PrimitiveType::F64) {
                    // Floats must keep their bit pattern: writeU32 would
                    // integer-truncate the value (and undersize f64 as 4 bytes).
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeF64(argsPtr, {});\n",
                        param.name
                    ));
                } else if matches!(p, PrimitiveType::F32) {
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeF32(argsPtr, {});\n",
                        param.name
                    ));
                } else {
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr, {});\n",
                        param.name
                    ));
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, {}.lo);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 4, {}.hi);\n",
                    param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_)
                if js_struct_for_type(&param.ty, &ir.types).is_some() =>
            {
                // Struct-by-value param: allocate its full C-layout size and pack
                // field-by-field (matches the Deno caller and every host thunk).
                let size: usize = js_c_size(&param.ty, ir)?;
                out.push_str(&format!("        const _argsBuf = _callerAlloc({size});\n"));
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                let mut sv_idx: u32 = 0;
                emit_ts_caller_pack_value(
                    out,
                    &param.ty,
                    "argsPtr",
                    0,
                    &param.name,
                    ir,
                    &mut sv_idx,
                )?;
            }
            ResolvedTypeRef::UserDefined(_) => {
                // Enum-backed scalar param. Pack through the repr width in an
                // 8-byte aligned slot: a plain writeU32 would truncate a u64-repr
                // enum's high word. The callee reads exactly its repr width from
                // offset 0 (little-endian), so narrower reprs are covered by the
                // low bytes of the u32 write.
                out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                match js_enum_for_type(&param.ty, enums).map(|e: &EnumDef| &e.repr) {
                    Some(ReprType::U64) => {
                        out.push_str(&format!(
                            "        polyplug.writeU32(argsPtr, Number({0}) >>> 0);\n",
                            param.name
                        ));
                        out.push_str(&format!(
                            "        polyplug.writeU32(argsPtr + 4, Math.floor(Number({0}) / 4294967296));\n",
                            param.name
                        ));
                    }
                    _ => {
                        out.push_str(&format!(
                            "        polyplug.writeU32(argsPtr, Number({0}));\n",
                            param.name
                        ));
                        out.push_str("        polyplug.writeU32(argsPtr + 4, 0);\n");
                    }
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const argsPtr = 0;\n");
            }
        }
        return Ok(());
    }

    // Multiple params: pack with C-layout (repr(C)) offsets. Every host-side
    // thunk unpacks the args through a natural-alignment struct (ctypes /
    // LuaJIT cdef / #[repr(C)] / LayoutKind.Sequential), so the pack must
    // match byte-for-byte — e.g. a u32 followed by a StringView places the
    // view at offset 8 (4 bytes of padding), not offset 4.
    let mut total_size: usize = 0;
    let mut max_align: usize = 1;
    for param in &func.params {
        let (size, align): (usize, usize) = js_caller_slot_layout(&param.ty, ir)?;
        if align > max_align {
            max_align = align;
        }
        total_size = align_up(total_size, align) + size;
    }
    total_size = align_up(total_size, max_align);

    out.push_str(&format!(
        "        const _argsBuf = _callerAlloc({});\n",
        total_size
    ));
    out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");

    let mut offset: usize = 0;
    let mut sv_idx: u32 = 0;
    for param in &func.params {
        let (size, align): (usize, usize) = js_caller_slot_layout(&param.ty, ir)?;
        offset = align_up(offset, align);
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                emit_ts_write_string_view(out, &param.name, &param.name, "argsPtr", offset);
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                emit_ts_write_buffer(out, &param.name, &param.name, "argsPtr", offset);
            }
            ResolvedTypeRef::Primitive(p) => match p {
                PrimitiveType::U64 | PrimitiveType::I64 => {
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, {}.lo);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, {}.hi);\n",
                        js_ptr_at("argsPtr", offset + 4),
                        param.name
                    ));
                }
                PrimitiveType::F64 => {
                    // Floats keep their bit pattern (writeU32 would truncate)
                    // and f64 occupies a full 8-byte slot in the args pack.
                    out.push_str(&format!(
                        "        polyplug.writeF64({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::F32 => {
                    out.push_str(&format!(
                        "        polyplug.writeF32({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::I32 => {
                    out.push_str(&format!(
                        "        polyplug.writeI32({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::U32 => {
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::U8 | PrimitiveType::I8 => {
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, {} & 0xFF);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::U16 | PrimitiveType::I16 => {
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, {} & 0xFF);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, ({} >> 8) & 0xFF);\n",
                        js_ptr_at("argsPtr", offset + 1),
                        param.name
                    ));
                }
                PrimitiveType::Bool => {
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, {} ? 1 : 0);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
            },
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str(&format!(
                    "        polyplug.writeU32({}, {}.lo);\n",
                    js_ptr_at("argsPtr", offset),
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32({}, {}.hi);\n",
                    js_ptr_at("argsPtr", offset + 4),
                    param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                match js_enum_for_type(&param.ty, enums) {
                    Some(e) => match e.repr {
                        ReprType::U8 => {
                            out.push_str(&format!(
                                "        polyplug.writeByte({}, Number({}) & 0xFF);\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                        }
                        ReprType::U16 => {
                            out.push_str(&format!(
                                "        polyplug.writeByte({}, Number({}) & 0xFF);\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                            out.push_str(&format!(
                                "        polyplug.writeByte({}, (Number({}) >> 8) & 0xFF);\n",
                                js_ptr_at("argsPtr", offset + 1),
                                param.name
                            ));
                        }
                        ReprType::U32 => {
                            out.push_str(&format!(
                                "        polyplug.writeU32({}, Number({}));\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                        }
                        ReprType::U64 => {
                            out.push_str(&format!(
                                "        polyplug.writeU32({}, Number({}) >>> 0);\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                            out.push_str(&format!(
                                "        polyplug.writeU32({}, Math.floor(Number({}) / 4294967296));\n",
                                js_ptr_at("argsPtr", offset + 4),
                                param.name
                            ));
                        }
                    },
                    None => {
                        // Struct-by-value caller param: pack field-by-field at its
                        // C-layout offsets (matches the Deno caller and host thunks).
                        emit_ts_caller_pack_value(
                            out,
                            &param.ty,
                            "argsPtr",
                            offset,
                            &param.name,
                            ir,
                            &mut sv_idx,
                        )?;
                    }
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
        }
        offset += size;
    }
    Ok(())
}

/// Emit the outPtr setup for a TypeScript guest host contract / peer method.
///
/// Allocates the correct size via `_callerAlloc` (host alloc, freed by the
/// method's `finally`) and defines `const outPtr`.
/// Does NOT pre-create `result` — that is done by `emit_ts_guest_host_contract_readback`
/// after the dispatch call succeeds.
fn emit_ts_guest_host_contract_out_setup(
    out: &mut String,
    returns: &Option<ResolvedTypeRef>,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    if let Some(ret_ty) = returns {
        match ret_ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str("        const _outBuf = _callerAlloc(16);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 8, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 12, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str("        const _outBuf = _callerAlloc(24);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 8, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 12, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 16, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 20, 0);\n");
            }
            ResolvedTypeRef::UserDefined(_) if js_struct_for_type(ret_ty, &ir.types).is_some() => {
                // Struct-by-value return: allocate the exact C-layout size and
                // zero the slot word-by-word (arena memory is not pre-zeroed); the
                // host writes each field, and the readback reads them at their C
                // offsets via js_read_expr.
                let size: usize = js_c_size(ret_ty, ir)?;
                out.push_str(&format!("        const _outBuf = _callerAlloc({size});\n"));
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                for w in 0..size.div_ceil(4) {
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, 0);\n",
                        js_ptr_at("outPtr", w * 4)
                    ));
                }
            }
            ResolvedTypeRef::UserDefined(_) => {
                // Enum-backed return: 8-byte slot, pre-zeroed; the host writes the
                // repr-width integer and the readback reads it back.
                out.push_str("        const _outBuf = _callerAlloc(8);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str("        const _outBuf = _callerAlloc(8);\n");
                    out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                    out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                    out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                } else {
                    // Allocate 8 bytes for safety even though the value is 4 bytes.
                    out.push_str("        const _outBuf = _callerAlloc(8);\n");
                    out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                    out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                    out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str("        const _outBuf = _callerAlloc(8);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const outPtr = 0;\n");
            }
        }
    } else {
        out.push_str("        const outPtr = 0;\n");
    }
    Ok(())
}

/// Emit `const result = ...;` by reading the dispatch result back from `outPtr`.
///
/// Called after the dispatch call succeeds (errCode === 0).  The `returns` value
/// must NOT be `None` or `Void` — callers must guard against that.
fn emit_ts_guest_host_contract_readback(
    out: &mut String,
    returns: Option<&ResolvedTypeRef>,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let enums: &[EnumDef] = &ir.enums;
    let Some(ret_ty) = returns else {
        return Ok(());
    };
    match ret_ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            out.push_str("        const result = { ptr_lo: polyplug.readU32(outPtr), ptr_hi: polyplug.readU32(outPtr + 4), len: polyplug.readU32(outPtr + 8) };\n");
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            out.push_str("        const result = { ptr_lo: polyplug.readU32(outPtr), ptr_hi: polyplug.readU32(outPtr + 4), len: polyplug.readU32(outPtr + 8), cap: polyplug.readU32(outPtr + 16) };\n");
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(
                "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) };\n",
            );
        }
        ResolvedTypeRef::Primitive(p) => match p {
            PrimitiveType::U64 | PrimitiveType::I64 => {
                out.push_str(
                    "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) };\n",
                );
            }
            PrimitiveType::I32 => {
                out.push_str("        const result: number = polyplug.readI32(outPtr);\n");
            }
            PrimitiveType::I8 => {
                // The host writes 1 byte into the (pre-zeroed) out slot; a
                // 32-bit read loses the sign — sign-extend the low byte.
                out.push_str(
                    "        const result: number = ((polyplug.readByte(outPtr) << 24) >> 24);\n",
                );
            }
            PrimitiveType::I16 => {
                out.push_str(
                    "        const result: number = (((polyplug.readByte(outPtr) | (polyplug.readByte(outPtr + 1) << 8)) << 16) >> 16);\n",
                );
            }
            PrimitiveType::F32 => {
                out.push_str("        const result: number = polyplug.readF32(outPtr);\n");
            }
            PrimitiveType::F64 => {
                out.push_str("        const result: number = polyplug.readF64(outPtr);\n");
            }
            PrimitiveType::Bool => {
                out.push_str("        const result: number = polyplug.readU32(outPtr);\n");
            }
            _ => {
                out.push_str("        const result: number = polyplug.readU32(outPtr);\n");
            }
        },
        ResolvedTypeRef::UserDefined(_) if js_struct_for_type(ret_ty, &ir.types).is_some() => {
            // Struct-by-value return: read field-by-field from outPtr into a
            // (possibly nested) object literal via js_read_expr — the same reader
            // the guest wrappers use, so the shape matches the raw return type.
            let expr: String = js_read_expr(ret_ty, "outPtr", 0, ir)?;
            out.push_str(&format!("        const result = {expr};\n"));
        }
        ResolvedTypeRef::UserDefined(_) => {
            // Enum returns read back as their repr integer — the declared TS
            // return type is the numeric enum itself, NOT the {lo, hi} object.
            // The out slot is pre-zeroed by the out-setup, so narrower reprs
            // read correctly through readU32.
            match js_enum_for_type(ret_ty, enums).map(|e: &EnumDef| &e.repr) {
                Some(ReprType::U64) => {
                    out.push_str(
                        "        const result = (polyplug.readU32(outPtr) + polyplug.readU32(outPtr + 4) * 4294967296) as any;\n",
                    );
                }
                Some(_) => {
                    out.push_str("        const result = polyplug.readU32(outPtr) as any;\n");
                }
                None => {
                    out.push_str(
                        "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) } as any;\n",
                    );
                }
            }
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
    }
    Ok(())
}

/// Generate `guest/host_contracts.ts` — caller classes for guest-side host contract callers.
fn generate_guest_host_contracts_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (guest-side callers)\n\n",
    );
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");

    let type_imports: BTreeSet<String> = collect_ts_guest_host_contract_type_imports(ir);
    if !type_imports.is_empty() {
        let import_list: String = type_imports.into_iter().collect::<Vec<String>>().join(", ");
        let entries: Vec<ImportEntry> = import_list
            .split(", ")
            .map(|s: &str| js_named(s, "./types"))
            .collect();
        out.push_str(&js_import_block(&[&entries]));
        out.push('\n');
    }
    emit_ts_utf8_encoder_helper(&mut out)?;

    for contract in &ir.host_contracts {
        generate_ts_guest_host_contract_caller(&mut out, contract, ir)?;
    }

    // Emit contract ID constants
    out.push_str("// Contract ID constants\n");
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_ts_caller(&contract.name);
        let const_name: String = class_name.to_uppercase() + "_ID";
        out.push_str(&format!(
            "/** Contract ID for `{}` (FNV-1a of \"host_contract:{}@{}\") */\n",
            contract.name, contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "export const {} = 0x{:016X}n;\n\n",
            const_name, contract.contract_id
        ));
    }

    Ok(out)
}

/// Collect user-defined type names (enums, structs) referenced in guest-side
/// host-contract caller signatures, so `host_contracts.ts` can import them from
/// `./types`. Without this, a host-contract method that takes an enum parameter
/// (e.g. `log_with_level(level: LogLevel, ...)`) references an undeclared name
/// and fails `deno check`.
fn collect_ts_guest_host_contract_type_imports(ir: &ValidatedIr) -> BTreeSet<String> {
    let mut imports: BTreeSet<String> = BTreeSet::new();
    for contract in &ir.host_contracts {
        for func in &contract.functions {
            for param in &func.params {
                if let ResolvedTypeRef::UserDefined(name) = &param.ty {
                    imports.insert(name.clone());
                }
            }
            if let Some(ResolvedTypeRef::UserDefined(name)) = &func.returns {
                imports.insert(name.clone());
            }
        }
    }
    imports
}

// ─── Host Interface Factories Generation ────────────────────────────────────────

/// Generate all host-side interface factories into a single file.
fn generate_js_host_interface_factories_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side interface factories)\n\n",
    );

    out.push_str(&js_import_block(&[&[
        js_named("buildHostContractInterface", "polyplug"),
        ImportEntry::JsTypeNamed {
            name: "Runtime".to_string(),
            source: "polyplug".to_string(),
        },
        ImportEntry::JsTypeNamespace {
            alias: "contracts".to_string(),
            source: "./contracts".to_string(),
        },
    ]]));
    out.push('\n');

    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("};\n\n");

    // Shared by the generated arg/return marshalling (StringView encode/decode).
    out.push_str("const _encoder = new TextEncoder();\n");
    out.push_str("const _decoder = new TextDecoder();\n\n");

    for contract in &ir.host_contracts {
        generate_js_host_interface_factory(&mut out, contract, ir)?;
    }

    Ok(out)
}

/// Generate the per-contract host interface factory.
///
/// Emits a factory-based provider: `create<Iface>Vtable(factory)` builds a real
/// C `HostContractInterface` (native dispatch via `Deno.UnsafeCallback`) with
/// per-instance state through the SDK's `buildHostContractInterface`. The
/// `factory` constructs a fresh implementation per instance — no module-level
/// impl storage (Rule 12). Each method is emitted as a thunk that reads its
/// packed args at C-layout offsets and calls the impl.
fn generate_js_host_interface_factory(
    out: &mut String,
    contract: &ResolvedHostContract,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
    let factory_name: String = format!("create{}Vtable", iface_name);
    let contract_id: u64 = contract.contract_id;
    let contract_id_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract_id >> 32) as u32;
    let major: u32 = contract.version.major;
    let minor: u32 = contract.version.minor;
    let singleton: bool = contract.singleton;

    out.push_str(&format!(
        "/**\n \
         * Build the host contract interface for `{}` (native dispatch, per-instance).\n \
         * `factory` builds a fresh implementation per instance; the runtime calls it\n \
         * once per non-singleton caller (independent state) or once for a singleton.\n \
         */\n",
        contract.name
    ));
    out.push_str(&format!(
        "export function {factory_name}(rt: Runtime, factory: () => contracts.{iface_name}) {{\n"
    ));
    out.push_str("    return buildHostContractInterface({\n");
    out.push_str(&format!("        contractIdLo: 0x{contract_id_lo:08X},\n"));
    out.push_str(&format!("        contractIdHi: 0x{contract_id_hi:08X},\n"));
    out.push_str(&format!("        major: {major},\n"));
    out.push_str(&format!("        minor: {minor},\n"));
    out.push_str(&format!("        singleton: {singleton},\n"));
    out.push_str("        factory,\n");
    out.push_str("        methods: [\n");
    for func in &contract.functions {
        generate_js_host_method_thunk(out, func, &iface_name, ir)?;
    }
    out.push_str("        ],\n");
    out.push_str("    });\n");
    out.push_str("}\n\n");
    Ok(())
}

/// Emit one native-dispatch thunk for a host contract method, as an entry in the
/// `methods` array passed to `buildHostContractInterface`. The thunk receives the
/// resolved impl and the packed-args pointer, reads each argument at its
/// natural-alignment (C-layout) offset — matching how the guest caller packs them
/// — calls the impl method, and returns `AbiErrorCode.Ok`.
fn generate_js_host_method_thunk(
    out: &mut String,
    func: &ResolvedFunction,
    iface_name: &str,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let method_name: String = host_method_pascal_case(&func.name);

    out.push_str(&format!(
        "            (impl: contracts.{iface_name}, argsPtr: Deno.PointerValue, outPtr: Deno.PointerValue): number => {{\n"
    ));

    // Read every argument at its natural-alignment (C-layout) offset — the exact
    // packing the guest caller produced — using the shared full-universe reader.
    // `owns_payload = false`: the args belong to the caller, so StringView/Buffer
    // payloads are borrowed (copied out, never freed).
    let mut arg_names: Vec<String> = Vec::new();
    if !func.params.is_empty() {
        let args_size: usize = deno_args_total_size(func, ir)?;
        out.push_str(&format!(
            "                const _argsDv = new DataView(new Deno.UnsafePointerView(argsPtr!).getArrayBuffer({args_size}));\n"
        ));
        let mut read_idx: u32 = 0;
        let mut offset: usize = 0;
        for param in &func.params {
            let align: usize = js_c_align(&param.ty, ir)?;
            offset = align_up(offset, align);
            let local: String =
                emit_deno_read_local(out, &param.ty, "_argsDv", offset, ir, &mut read_idx, false)?;
            arg_names.push(local);
            offset += js_c_size(&param.ty, ir)?;
        }
    }

    match &func.returns {
        Some(ret_ty) => {
            out.push_str(&format!(
                "                const _result = impl.{method_name}({});\n",
                arg_names.join(", ")
            ));
            // Write the return through outPtr with the shared full-universe writer.
            // StringView/Buffer payloads are host-allocated here and freed later by
            // the guest caller, so the per-call `_allocs` list is intentionally NOT
            // freed by the provider (freeing it would be a use-after-free).
            let ret_size: usize = js_c_size(ret_ty, ir)?;
            out.push_str(&format!(
                "                const _outDv = new DataView(new Deno.UnsafePointerView(outPtr!).getArrayBuffer({ret_size}));\n"
            ));
            out.push_str("                const _allocs: [Deno.PointerValue, number][] = [];\n");
            let mut alloc_idx: u32 = 0;
            emit_deno_write_value(out, ret_ty, "_outDv", 0, "_result", ir, &mut alloc_idx)?;
            out.push_str("                void _allocs;\n");
        }
        None => {
            out.push_str(&format!(
                "                impl.{method_name}({});\n",
                arg_names.join(", ")
            ));
        }
    }
    out.push_str("                return AbiErrorCode.Ok;\n");
    out.push_str("            },\n");
    Ok(())
}

/// Convert a `snake_case` (or dotted) method name to the `PascalCase` form the
/// generated TypeScript host interface declares.
fn host_method_pascal_case(name: &str) -> String {
    name.split(['_', '.'])
        .filter(|seg: &&str| !seg.is_empty())
        .map(|seg: &str| {
            let mut chars: core::str::Chars<'_> = seg.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("")
}

// ─── Peer Caller Generation ───────────────────────────────────────────────────

/// Convert a guest contract name (e.g. `pipeline.Validator`) to a TypeScript
/// peer-caller class name (e.g. `ValidatorPeer`).
fn guest_contract_name_to_ts_peer(name: &str) -> String {
    let last: &str = name.split('.').next_back().unwrap_or(name);
    let mut chars: core::str::Chars<'_> = last.chars();
    let pascal: String = match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    pascal + "Peer"
}

/// Generate `guest/peer_callers.ts` — one typed peer class per matched dependency.
fn generate_guest_peer_callers_ts(
    ir: &ValidatedIr,
    peers: &[&ResolvedContract],
) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (guest-side peer callers)\n\n",
    );
    emit_js_attributes(&mut out, CustomizableNode::Api, &ir.langs, "");

    // Collect user-defined type imports needed by peer method signatures.
    let mut type_imports: BTreeSet<String> = BTreeSet::new();
    for contract in peers {
        for func in &contract.functions {
            for param in &func.params {
                if let ResolvedTypeRef::UserDefined(name) = &param.ty {
                    type_imports.insert(name.clone());
                }
            }
            if let Some(ResolvedTypeRef::UserDefined(name)) = &func.returns {
                type_imports.insert(name.clone());
            }
        }
    }
    if !type_imports.is_empty() {
        let import_list: String = type_imports.into_iter().collect::<Vec<String>>().join(", ");
        let entries: Vec<ImportEntry> = import_list
            .split(", ")
            .map(|s: &str| js_named(s, "./types"))
            .collect();
        out.push_str(&js_import_block(&[&entries]));
        out.push('\n');
    }
    emit_ts_utf8_encoder_helper(&mut out)?;

    for contract in peers {
        let min_ver: u32 = peer_min_version(ir, contract.contract_id);
        generate_ts_peer_caller_class(&mut out, contract, min_ver, ir)?;
    }

    Ok(out)
}

/// Generate one `<Name>Peer` class for a single peer contract.
fn generate_ts_peer_caller_class(
    out: &mut String,
    contract: &ResolvedContract,
    min_version: u32,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let class_name: String = guest_contract_name_to_ts_peer(&contract.name);
    let contract_id_lo: u32 = (contract.contract_id & 0xFFFF_FFFF) as u32;
    let contract_id_hi: u32 = (contract.contract_id >> 32) as u32;

    emit_js_attributes(out, CustomizableNode::GuestContract, &contract.langs, "");
    out.push_str(&format!(
        "/**\n * Peer caller for guest contract `{}` (id=0x{:016X})\n *\n\
         * Dispatches through the threaded `bridge.dispatchPeer` primitive.\n\
         *\n\
         * The loader resolves the peer interface and dispatches DIRECTLY through it,\n\
         * without re-entering the host to resolve the same interface a second time.\n\
         * Resolution is per-call here; the\n\
         * native-dispatch languages (rust/cpp/csharp/python/lua) additionally CACHE the\n\
         * resolved interface, which a QuickJS guest cannot (it cannot dereference raw\n\
         * pointers, so it reaches host capabilities ONLY through the threaded `bridge`,\n\
         * Rule 12). `dispatchPeer` is the bridge primitive that performs that loader-side\n\
         * resolve + direct dispatch on the guest's behalf. The revision snapshot +\n\
         * per-call staleness check below keep correctness parity with the cached callers.\n\
         *\n\
         * Uses per-call create+destroy (stateless contract model).\n\
         * Stateful peers would require a retained instance-handle API.\n */\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("export class {} {{\n", class_name));
    out.push_str(&format!(
        "    /** Contract ID for `{}@{}` (FNV-1a). */\n",
        contract.name, contract.version.major
    ));
    out.push_str(&format!(
        "    static readonly CONTRACT_ID_LO = 0x{:08X};\n",
        contract_id_lo
    ));
    out.push_str(&format!(
        "    static readonly CONTRACT_ID_HI = 0x{:08X};\n",
        contract_id_hi
    ));
    out.push_str(&format!(
        "    static readonly MIN_VERSION = {};\n\n",
        min_version
    ));

    // The bridge and host pointer are threaded in explicitly (no global — Rule 12)
    // and stored as instance state so every method reaches the host through them.
    out.push_str("    private _bridge: any;\n");
    out.push_str("    private _hostPtr: { lo: number; hi: number };\n");
    // Registry revision read when this peer was resolved (low/high f64 halves of the
    // runtime's u64 counter, since QuickJS numbers cannot carry a full u64). Compared
    // before each dispatch against the live counter to detect a reload/unload of the
    // peer and re-resolve, so a stale resolution is never dispatched through.
    out.push_str("    private _revLo: number;\n");
    out.push_str("    private _revHi: number;\n\n");

    out.push_str("    private constructor(bridge: any, hostPtr: { lo: number; hi: number }, revLo: number, revHi: number) {\n");
    out.push_str("        this._bridge = bridge;\n");
    out.push_str("        this._hostPtr = hostPtr;\n");
    out.push_str("        this._revLo = revLo;\n");
    out.push_str("        this._revHi = revHi;\n");
    out.push_str("    }\n\n");

    out.push_str("    /**\n");
    out.push_str("     * Verify the peer contract is reachable via the host.\n");
    out.push_str("     * Returns a `");
    out.push_str(&class_name);
    out.push_str("` instance or `null` if not found.\n");
    out.push_str("     *\n");
    out.push_str("     * `bridge` and `hostPtr` are threaded in explicitly by the caller\n");
    out.push_str("     * (the author factory captured them); no per-VM global is read.\n");
    out.push_str("     */\n");
    out.push_str(&format!(
        "    static resolve(bridge: any, hostPtr: {{ lo: number; hi: number }}): {} | null {{\n",
        class_name
    ));
    out.push_str("        if (!bridge || !bridge.findByContract) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        const handle = bridge.findByContract(0x{:08X}, 0x{:08X}, {});\n",
        contract_id_lo, contract_id_hi, min_version
    ));
    // Only null/undefined mean "not found": the loader's pack_handle already
    // returns null for the null handle, and slot 0 generation 0 legitimately
    // packs to 0 — testing `=== 0` would falsely reject a valid handle.
    out.push_str("        if (handle === null || handle === undefined) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    // Snapshot the registry revision at resolve time. The bridge reads the runtime's
    // monotonic counter (bumped on every load/reload/unload) and returns it as [lo, hi]
    // f64 halves. The loader always installs `revision` on the bridge, so call it directly.
    out.push_str("        const _rev = bridge.revision();\n");
    out.push_str(&format!(
        "        return new {}(bridge, hostPtr, _rev[0], _rev[1]);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    // Re-resolve this peer after the registry changed under us: a hot-reload swapped a
    // new interface into the same slot (findByContract still succeeds), an unload vacated
    // it (findByContract returns null — the peer is gone, return false). The actual
    // interface/instance are resolved fresh inside dispatchPeer on each dispatch, so
    // revalidation only re-confirms reachability and re-snapshots the revision.
    out.push_str("    private _revalidate(): boolean {\n");
    out.push_str("        if (!this._bridge || !this._bridge.findByContract) {\n");
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        const handle = this._bridge.findByContract(0x{:08X}, 0x{:08X}, {});\n",
        contract_id_lo, contract_id_hi, min_version
    ));
    out.push_str("        if (handle === null || handle === undefined) {\n");
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str("        const _rev = this._bridge.revision();\n");
    out.push_str("        this._revLo = _rev[0];\n");
    out.push_str("        this._revHi = _rev[1];\n");
    out.push_str("        return true;\n");
    out.push_str("    }\n\n");

    // True when the live registry revision differs from the one cached at resolve time.
    out.push_str("    private _revisionChanged(): boolean {\n");
    out.push_str("        if (!this._bridge) {\n");
    out.push_str("            return false;\n");
    out.push_str("        }\n");
    out.push_str("        const _rev = this._bridge.revision();\n");
    out.push_str("        return _rev[0] !== this._revLo || _rev[1] !== this._revHi;\n");
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_ts_peer_caller_method(out, func, contract_id_lo, contract_id_hi, min_version, ir)?;
    }

    out.push_str("}\n\n");
    Ok(())
}

/// Generate one method for a peer caller class.
///
/// Reuses `emit_ts_guest_host_contract_args_setup` and
/// `emit_ts_guest_host_contract_out_setup` for identical StringView/Buffer/u64
/// marshalling.  The call itself routes through `polyplug.dispatchPeer`
/// instead of reading a vtable header — that is the key simplification vs the
/// host-contract method.
fn generate_ts_peer_caller_method(
    out: &mut String,
    func: &ResolvedFunction,
    contract_id_lo: u32,
    contract_id_hi: u32,
    min_version: u32,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let fn_id: u32 = func.function_id;
    let has_return: bool = func.returns.is_some();
    // Use the raw ABI return type — it must match the `result` shape that
    // emit_ts_guest_host_contract_out_setup initialises (StringView → {ptr_lo,ptr_hi,len}, etc.).
    let return_type: String = match &func.returns {
        Some(ty) => ts_peer_raw_return_type(ty),
        None => "void".to_owned(),
    };

    let params_str: String = if func.params.is_empty() {
        String::new()
    } else {
        func.params
            .iter()
            .map(|p: &ResolvedParam| {
                let ts_ty: String = ts_guest_caller_param_type(&p.ty);
                format!("{}: {}", p.name, ts_ty)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    emit_js_function_attributes(out, func, "    ");
    out.push_str(&format!("    /** Call peer `{}` */\n", func.name));
    out.push_str(&format!(
        "    {}({}): {} {{\n",
        func.name, params_str, return_type
    ));

    out.push_str("        const polyplug = this._bridge;\n");
    out.push_str("        if (!polyplug || !polyplug.dispatchPeer) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");
    // Cheap per-call staleness check: if the registry revision changed since resolve
    // (hot-reload or unload of the peer), re-resolve first so a stale resolution is
    // never dispatched. A failed re-resolve means the peer is gone — throw.
    out.push_str("        if (this._revisionChanged() && !this._revalidate()) {\n");
    out.push_str(&format!(
        "            throw new Error(`peer call {} failed: contract gone after reload/unload`);\n",
        func.name
    ));
    out.push_str("        }\n");
    emit_ts_caller_alloc_shim(out);
    out.push_str("        try {\n");

    // Marshal args and out buffer using the same helpers as host-contract methods.
    emit_ts_guest_host_contract_args_setup(out, func, ir)?;
    emit_ts_guest_host_contract_out_setup(out, &func.returns, ir)?;

    // Call the bridge primitive.  It returns the u32 error code directly.
    out.push_str(&format!(
        "        const errCode: number = polyplug.dispatchPeer(0x{:08X}, 0x{:08X}, {}, {}, argsPtr, outPtr);\n",
        contract_id_lo, contract_id_hi, min_version, fn_id
    ));
    // Throw with the ABI error code (JS host SDK convention) instead of
    // silently returning null and discarding the code.
    out.push_str("        if (errCode !== 0) {\n");
    out.push_str(&format!(
        "            throw new Error(`peer call {} failed (code ${{errCode}})`);\n",
        func.name
    ));
    out.push_str("        }\n");

    if has_return {
        emit_ts_guest_host_contract_readback(out, func.returns.as_ref(), ir)?;
        out.push_str("        return result;\n");
    }

    emit_ts_caller_free_shim(out);

    out.push_str("    }\n\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::ResolvedBundleFile;
    use crate::ir::ResolvedDependency;
    use crate::ir::Version;
    use crate::ir::{LanguageAttributes, LanguageRules};
    use crate::{Lang, OutputDestination, OutputPartition, ValidatedImport};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::tempdir;

    fn js_attribute_rules(attribute: &str) -> LanguageRules {
        LanguageRules {
            javascript: Some(LanguageAttributes {
                attributes: vec![attribute.to_owned()],
            }),
            ..LanguageRules::default()
        }
    }

    fn js_semantic_attribute_ir() -> ValidatedIr {
        let function = || ResolvedFunction {
            name: "measure".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "sample".to_owned(),
                ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                docs: None,
                langs: js_attribute_rules("param_marker"),
            }],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
            langs: js_attribute_rules("function_marker"),
            return_langs: js_attribute_rules("return_marker"),
        };

        ValidatedIr {
            types: vec![ResolvedType {
                name: "Packet".to_owned(),
                fields: vec![ResolvedField {
                    name: "count".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: js_attribute_rules("field_marker"),
                }],
                docs: None,
                langs: js_attribute_rules("type_marker"),
            }],
            enums: vec![EnumDef {
                name: "Status".to_owned(),
                repr: ReprType::U32,
                bitflag: false,
                variants: vec![EnumVariant {
                    name: "Ready".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                    langs: js_attribute_rules("variant_marker"),
                }],
                docs: None,
                langs: js_attribute_rules("enum_marker"),
            }],
            contracts: vec![ResolvedContract {
                name: "sample.measure".to_owned(),
                contract_id: 0x10,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![function()],
                docs: None,
                langs: js_attribute_rules("guest_marker"),
            }],
            host_contracts: vec![ResolvedHostContract {
                name: "host.measure".to_owned(),
                contract_id: 0x11,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![function()],
                docs: None,
                langs: js_attribute_rules("host_marker"),
            }],
            bundle: None,
            langs: js_attribute_rules("api_marker"),
        }
    }

    #[test]
    fn js_attributes_cover_public_semantic_surfaces_in_unified_and_split_outputs() {
        let ir = js_semantic_attribute_ir();

        let types = generate_types_ts(&ir).expect("render unified types");
        for marker in [
            "api_marker",
            "type_marker",
            "field_marker",
            "enum_marker",
            "variant_marker",
            "guest_marker",
            "function_marker",
            "param_marker",
            "return_marker",
        ] {
            assert!(
                types.contains(marker),
                "unified types must retain the `{marker}` semantic marker: {types}"
            );
        }
        assert!(
            types.find("function_marker") < types.find("param_marker")
                && types.find("param_marker") < types.find("return_marker"),
            "function, parameter, and return metadata must remain in authored order: {types}"
        );

        let host_callers = generate_callers_ts(&ir).expect("render host callers");
        assert!(
            host_callers.contains("guest_marker")
                && host_callers.contains("function_marker")
                && host_callers.contains("param_marker")
                && host_callers.contains("return_marker"),
            "host callers must project guest contract metadata: {host_callers}"
        );

        let host_contracts = generate_host_contracts_ts(&ir);
        assert!(
            host_contracts.contains("host_marker")
                && host_contracts.contains("function_marker")
                && host_contracts.contains("param_marker")
                && host_contracts.contains("return_marker"),
            "host contract interfaces must project semantic metadata: {host_contracts}"
        );

        let guest_host = generate_guest_host_contracts_ts(&ir).expect("render guest host callers");
        assert!(
            guest_host.contains("host_marker")
                && guest_host.contains("function_marker")
                && guest_host.contains("param_marker")
                && guest_host.contains("return_marker"),
            "guest host callers must project host contract metadata: {guest_host}"
        );

        let mut peer = String::new();
        generate_ts_peer_caller_class(&mut peer, &ir.contracts[0], 0, &ir)
            .expect("render peer caller");
        assert!(
            peer.contains("guest_marker")
                && peer.contains("function_marker")
                && peer.contains("param_marker")
                && peer.contains("return_marker"),
            "peer callers must project guest contract metadata: {peer}"
        );

        let host_factories =
            generate_js_host_interface_factories_ts(&ir).expect("render ABI host factories");
        assert!(
            !host_factories.contains("_marker"),
            "semantic JSDoc must not decorate ABI interface objects or wrapper glue: {host_factories}"
        );

        let layout = OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: PathBuf::from("domain"),
                import: ValidatedImport::parse(Lang::JsQuickJs, "./domain/types.ts")
                    .expect("valid JavaScript domain import"),
            },
            guest_contracts: OutputDestination::Emit {
                root: PathBuf::from("contracts"),
                import: ValidatedImport::parse(Lang::JsQuickJs, "./contracts.ts")
                    .expect("valid JavaScript contracts import"),
            },
        };
        let generator = JsQuickjsGenerator;
        let mut split_guest = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &layout, &mut split_guest)
            .expect("generate split guest");
        let split_domain = split_guest
            .files
            .iter()
            .find(|file| file.path.as_path() == Path::new("guest/types.ts"))
            .expect("split domain types");
        let split_contracts = split_guest
            .files
            .iter()
            .find(|file| file.path.as_path() == Path::new("guest/contracts.ts"))
            .expect("split guest contracts");
        assert!(
            split_domain.content.contains("type_marker")
                && split_domain.content.contains("field_marker")
                && split_domain.content.contains("enum_marker")
                && split_domain.content.contains("variant_marker")
                && split_contracts.content.contains("guest_marker")
                && split_contracts.content.contains("function_marker")
                && split_contracts.content.contains("param_marker")
                && split_contracts.content.contains("return_marker"),
            "split output must retain domain and guest contract metadata:\ndomain={}\ncontracts={}",
            split_domain.content,
            split_contracts.content
        );

        let temp = tempdir().expect("temporary Deno project");
        fs::write(temp.path().join("types.ts"), &types).expect("write generated types");
        let check = Command::new("deno")
            .args(["check", "--quiet", "types.ts"])
            .current_dir(temp.path())
            .output()
            .expect("run generated types Deno check");
        assert!(
            check.status.success(),
            "semantic JSDoc must typecheck: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }

    #[test]
    fn empty_js_rules_preserve_generated_bytes() {
        let ir = |langs| ValidatedIr {
            types: Vec::new(),
            enums: Vec::new(),
            contracts: Vec::new(),
            host_contracts: Vec::new(),
            bundle: None,
            langs,
        };
        let default = ir(LanguageRules::default());
        let explicit_empty = ir(LanguageRules {
            javascript: Some(LanguageAttributes::default()),
            ..LanguageRules::default()
        });

        assert_eq!(
            generate_types_ts(&default).expect("render default types"),
            generate_types_ts(&explicit_empty).expect("render explicit empty rules"),
            "an explicit empty JavaScript rule must preserve generated bytes"
        );
        assert_eq!(
            generate_callers_ts(&default).expect("render default callers"),
            generate_callers_ts(&explicit_empty).expect("render explicit empty rules"),
            "an explicit empty JavaScript rule must preserve caller bytes"
        );
    }

    #[test]
    fn generate_js_quickjs_enum_non_bitflag() {
        let e: EnumDef = EnumDef {
            name: "PixelFormat".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![
                EnumVariant {
                    name: "Unknown".to_owned(),
                    value: "0".to_owned(),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                EnumVariant {
                    name: "Rgba8".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_js_quickjs_enum(&mut out, &e).expect("render enum");
        assert!(
            out.contains("Object.freeze({"),
            "missing Object.freeze: {out}"
        );
        assert!(out.contains("Unknown: 0"), "missing Unknown: {out}");
        assert!(
            !out.contains("@bitflag"),
            "non-bitflag should not have @bitflag: {out}"
        );
    }

    #[test]
    fn generate_js_quickjs_enum_bitflag() {
        let e: EnumDef = EnumDef {
            name: "ImageFlags".to_owned(),
            repr: ReprType::U32,
            bitflag: true,
            variants: vec![
                EnumVariant {
                    name: "None".to_owned(),
                    value: "0".to_owned(),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                EnumVariant {
                    name: "Compressed".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_js_quickjs_enum(&mut out, &e).expect("render enum");
        assert!(out.contains("@bitflag"), "missing @bitflag: {out}");
        assert!(
            out.contains("Object.freeze({"),
            "missing Object.freeze: {out}"
        );
    }

    // ─── Host Contract Interface Tests ────────────────────────────────────────

    #[test]
    fn host_contract_name_to_ts_interface_conversion() {
        assert_eq!(
            host_contract_name_to_ts_interface("host.logger"),
            "HostLogger"
        );
        assert_eq!(
            host_contract_name_to_ts_interface("host.fs.reader"),
            "HostFsReader"
        );
        assert_eq!(
            host_contract_name_to_ts_interface("host.HostLogger"),
            "HostLogger"
        );
        assert_eq!(host_contract_name_to_ts_interface("logger"), "HostLogger");
    }

    #[test]
    fn ts_host_param_type_mappings() {
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U64)),
            "{ lo: number; hi: number }"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn ts_host_return_type_mappings() {
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn generate_ts_host_contract_interface_produces_interface() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x1234_5678_9ABC_DEF0_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![
                ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                        langs: LanguageRules::default(),
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                },
                ResolvedFunction {
                    name: "logf".to_owned(),
                    function_id: 1,
                    params: vec![
                        ResolvedParam {
                            name: "level".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                        ResolvedParam {
                            name: "format".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                    ],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_ts_host_contract_interface(&mut out, &contract);
        assert!(
            out.contains("export interface HostLogger"),
            "missing interface: {out}"
        );
        assert!(
            out.contains("Log(message: string): void"),
            "missing Log method: {out}"
        );
        assert!(
            out.contains("Logf(level: number, format: string): void"),
            "missing Logf method: {out}"
        );
    }

    #[test]
    fn generate_host_contracts_ts_produces_file() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                        langs: LanguageRules::default(),
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let out: String = generate_host_contracts_ts(&ir);
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(
            out.contains("export interface HostLogger"),
            "missing interface: {out}"
        );
        assert!(
            out.contains("HOSTLOGGER_CONTRACT_ID"),
            "missing constant: {out}"
        );
        assert!(
            out.contains("0x123456789ABCDEF0n"),
            "missing contract ID value: {out}"
        );
    }

    #[test]
    fn generate_host_with_host_contracts_produces_contracts_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_host(&ir, &OutputLayout::unified(), &mut files)
            .expect("generate_host");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            names.contains(&"host/contracts.ts".to_owned()),
            "missing host/contracts.ts: {names:?}"
        );
    }

    #[test]
    fn generate_host_without_host_contracts_no_contracts_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_host(&ir, &OutputLayout::unified(), &mut files)
            .expect("generate_host");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            !names.contains(&"host/contracts.ts".to_owned()),
            "unexpected host/contracts.ts: {names:?}"
        );
    }

    // ─── Guest Host Contract Caller Tests ─────────────────────────────────────

    #[test]
    fn host_contract_name_to_ts_caller_conversion() {
        assert_eq!(
            host_contract_name_to_ts_caller("host.logger"),
            "HostLoggerContract"
        );
        assert_eq!(
            host_contract_name_to_ts_caller("host.fs.reader"),
            "HostFsReaderContract"
        );
        assert_eq!(
            host_contract_name_to_ts_caller("host.HostLogger"),
            "HostLoggerContract"
        );
        assert_eq!(
            host_contract_name_to_ts_caller("logger"),
            "HostLoggerContract"
        );
    }

    #[test]
    fn ts_guest_caller_param_type_mappings() {
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U64)),
            "{ lo: number; hi: number }"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn generate_ts_guest_host_contract_caller_produces_class() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x1234_5678_9ABC_DEF0_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![
                ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                        langs: LanguageRules::default(),
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                },
                ResolvedFunction {
                    name: "logf".to_owned(),
                    function_id: 1,
                    params: vec![
                        ResolvedParam {
                            name: "level".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                        ResolvedParam {
                            name: "format".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                    ],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("export class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains(
                "private constructor(bridge: any, hostPtr: { lo: number; hi: number }, minVersion: number)"
            ),
            "missing private constructor: {out}"
        );
        assert!(
            out.contains(
                "static fromHost(bridge: any, hostPtr: { lo: number; hi: number }, minVersion: number = 0)"
            ),
            "missing fromHost: {out}"
        );
        assert!(out.contains("isValid(): boolean"), "missing isValid: {out}");
        // The bridge is threaded in, not read from a global (Rule 12).
        assert!(
            !out.contains("(globalThis as any).polyplug"),
            "host-contract caller must not read a global bridge: {out}"
        );
        assert!(
            out.contains("const polyplug = this._bridge;"),
            "host-contract caller methods must use the threaded bridge: {out}"
        );
        assert!(
            out.contains("log(message: string): void"),
            "missing log method: {out}"
        );
        assert!(
            out.contains("logf(level: number, format: string): void"),
            "missing logf method: {out}"
        );
        // New shape: uses callHostContract, arenaAlloc, not the old dispatch machinery.
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
        assert!(
            !out.contains("readHostContractHeader"),
            "must not use readHostContractHeader: {out}"
        );
        assert!(
            !out.contains("callDispatchFn"),
            "must not use callDispatchFn: {out}"
        );
    }

    #[test]
    fn generate_ts_guest_host_contract_caller_with_return() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.fs.reader".to_owned(),
            contract_id: 0xDEAD_BEEF_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "read".to_owned(),
                function_id: 0,
                params: vec![ResolvedParam {
                    name: "path".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    docs: None,
                    langs: LanguageRules::default(),
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
                docs: None,
                return_docs: None,
                langs: LanguageRules::default(),
                return_langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("export class HostFsReaderContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains(
                "read(path: string): { ptr_lo: number; ptr_hi: number; len: number; cap: number }"
            ),
            "missing read method with raw Buffer return: {out}"
        );
    }

    #[test]
    fn generate_guest_host_contracts_ts_produces_file() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                        langs: LanguageRules::default(),
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let out: String =
            generate_guest_host_contracts_ts(&ir).expect("generate guest host contracts");
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(
            out.contains("export class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains("HOSTLOGGERCONTRACT_ID"),
            "missing constant: {out}"
        );
        assert!(
            out.contains("0x123456789ABCDEF0n"),
            "missing contract ID value: {out}"
        );
    }

    #[test]
    fn generate_guest_with_host_contracts_produces_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &OutputLayout::unified(), &mut files)
            .expect("generate_guest");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            names.contains(&"guest/host_contracts.ts".to_owned()),
            "missing guest/host_contracts.ts: {names:?}"
        );
    }

    #[test]
    fn enum_is_exported_for_cross_module_use() {
        // Regression: `host_contracts.ts` references enum types (e.g. `LogLevel`)
        // declared in `types.ts`. The enum const and its type must be `export`ed
        // or the cross-module reference fails `deno check` with TS2304.
        let e: EnumDef = EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Debug".to_owned(),
                value: "0".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_js_quickjs_enum(&mut out, &e).expect("render enum");
        assert!(
            out.contains("export const LogLevel = Object.freeze({"),
            "enum const must be exported: {out}"
        );
        assert!(
            out.contains("export type LogLevel = typeof LogLevel"),
            "enum type must be exported: {out}"
        );
    }

    #[test]
    fn guest_host_contracts_imports_enum_param_types() {
        // Regression: a host-contract method taking an enum parameter must import
        // that enum from `./types`, otherwise the name is undeclared (TS2304).
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log_with_level".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "level".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                        docs: None,
                        langs: LanguageRules::default(),
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let out: String =
            generate_guest_host_contracts_ts(&ir).expect("generate guest host contracts");
        assert!(
            out.contains("import { LogLevel } from './types';"),
            "host_contracts.ts must import enum param type from ./types: {out}"
        );
    }

    #[test]
    fn guest_host_contracts_no_type_import_when_no_user_types() {
        // A host contract using only ABI builtins must NOT emit a spurious import.
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                        langs: LanguageRules::default(),
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                    langs: LanguageRules::default(),
                    return_langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let out: String =
            generate_guest_host_contracts_ts(&ir).expect("generate guest host contracts");
        assert!(
            !out.contains("from './types'"),
            "no type import expected for ABI-only host contract: {out}"
        );
    }

    #[test]
    fn generate_guest_without_host_contracts_no_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &OutputLayout::unified(), &mut files)
            .expect("generate_guest");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            !names.contains(&"guest/host_contracts.ts".to_owned()),
            "unexpected guest/host_contracts.ts: {names:?}"
        );
    }

    #[test]
    fn peer_caller_emitted_for_declared_dependency() {
        // Build an IR where the bundle declares a dependency on a contract that
        // IS present in ir.contracts — the generator must emit peer_callers.ts.
        let validator_id: u64 = guest_contract_id("pipeline.Validator", 1);
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![ResolvedContract {
                name: "pipeline.Validator".to_owned(),
                contract_id: validator_id,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![],
                docs: None,
                langs: LanguageRules::default(),
            }],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "transformer".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                loader: "js-quickjs".to_owned(),
                file: ResolvedBundleFile::Single("libtransformer.so".to_owned()),
                plugins: vec![],
                bundle_id: 0,
                dependencies: vec![ResolvedDependency::ByContract {
                    contract: "pipeline.Validator".to_owned(),
                    contract_id: validator_id,
                    min_version: 1 << 16,
                }],
                needs_reinit_on_dep_reload: false,
            }),
            langs: LanguageRules::default(),
        };
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &OutputLayout::unified(), &mut files)
            .expect("generate_guest");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            names.contains(&"guest/peer_callers.ts".to_owned()),
            "expected guest/peer_callers.ts in {names:?}"
        );
        // The generated file must dispatch through the bridge's `dispatchPeer`
        // primitive (direct loader-side dispatch), NOT the removed `callGuestMethod`.
        let peer_file: &GeneratedFile = files
            .files
            .iter()
            .find(|f: &&GeneratedFile| f.path.to_string_lossy() == "guest/peer_callers.ts")
            .expect("peer_callers.ts");
        assert!(
            peer_file.content.contains("dispatchPeer"),
            "peer_callers.ts must call dispatchPeer"
        );
        assert!(
            !peer_file.content.contains("callGuestMethod"),
            "peer_callers.ts must not reference the removed callGuestMethod bridge"
        );
        assert!(
            peer_file.content.contains("ValidatorPeer"),
            "peer_callers.ts must contain ValidatorPeer class"
        );
        // guest/index.ts must re-export ValidatorPeer.
        let index_file: &GeneratedFile = files
            .files
            .iter()
            .find(|f: &&GeneratedFile| f.path.to_string_lossy() == "guest/index.ts")
            .expect("guest/index.ts");
        assert!(
            index_file.content.contains("ValidatorPeer"),
            "guest/index.ts must re-export ValidatorPeer"
        );
    }

    #[test]
    fn guest_host_contract_stringview_return_uses_new_dispatch() {
        // A StringView-returning host-contract method must use _callerAlloc(16) (host
        // alloc, freed by the method's finally) for the out buffer, emit
        // polyplug.callHostContract, and read back via readU32(outPtr+8).
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.svc".to_owned(),
            contract_id: 0xABCD_1234_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "describe".to_owned(),
                function_id: 1,
                params: vec![ResolvedParam {
                    name: "key".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    docs: None,
                    langs: LanguageRules::default(),
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                docs: None,
                return_docs: None,
                langs: LanguageRules::default(),
                return_langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("const _outBuf = _callerAlloc(16);"),
            "out buffer must be _callerAlloc(16) for StringView: {out}"
        );
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
        assert!(
            out.contains("polyplug.readU32(outPtr + 8)"),
            "must read len from outPtr+8: {out}"
        );
        // Caller buffers are host-allocated and explicitly freed (no global arena).
        assert!(
            out.contains("for (const _f of _frees) { polyplug.free("),
            "caller must free its host-allocated buffers: {out}"
        );
    }

    #[test]
    fn guest_host_contract_buffer_out_uses_arena_alloc_24() {
        // A Buffer-returning host-contract method must allocate 24 bytes for the out slot.
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.store".to_owned(),
            contract_id: 0x1111_2222_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "snapshot".to_owned(),
                function_id: 2,
                params: vec![],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
                docs: None,
                return_docs: None,
                langs: LanguageRules::default(),
                return_langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("const _outBuf = _callerAlloc(24);"),
            "out buffer must be _callerAlloc(24) for Buffer: {out}"
        );
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
    }

    #[test]
    fn no_peer_callers_without_dependencies() {
        // A bundle with no [[dependency]] entries must NOT emit peer_callers.ts.
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![ResolvedContract {
                name: "pipeline.Validator".to_owned(),
                contract_id: polyplug_utils::guest_contract_id("pipeline.Validator", 1),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![],
                docs: None,
                langs: LanguageRules::default(),
            }],
            host_contracts: vec![],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &OutputLayout::unified(), &mut files)
            .expect("generate_guest");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            !names.contains(&"guest/peer_callers.ts".to_owned()),
            "unexpected guest/peer_callers.ts: {names:?}"
        );
    }

    fn canonical_contract_layout_ir() -> ValidatedIr {
        let contract: ResolvedContract = ResolvedContract {
            name: "test.add".to_owned(),
            contract_id: guest_contract_id("test.add", 1),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![ResolvedFunction {
                name: "add".to_owned(),
                function_id: 0,
                params: vec![ResolvedParam {
                    name: "args".to_owned(),
                    ty: ResolvedTypeRef::UserDefined("AddArgs".to_owned()),
                    docs: None,
                    langs: LanguageRules::default(),
                }],
                returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                docs: None,
                return_docs: None,
                langs: LanguageRules::default(),
                return_langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        };
        ValidatedIr {
            types: vec![ResolvedType {
                name: "AddArgs".to_owned(),
                fields: vec![
                    ResolvedField {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                    ResolvedField {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            }],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "test_js_bundle".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                loader: "js-quickjs".to_owned(),
                file: ResolvedBundleFile::Single("plugin.js".to_owned()),
                plugins: vec![ResolvedPlugin {
                    name: "test_adder".to_owned(),
                    implements: vec!["test.add@1.0".to_owned()],
                    optional: vec![],
                }],
                bundle_id: 0,
                dependencies: vec![],
                needs_reinit_on_dep_reload: false,
            }),
            langs: LanguageRules::default(),
        }
    }

    #[test]
    fn imported_guest_contract_aliases_define_quickjs_provider_surface() {
        let layout: OutputLayout = OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::JsQuickJs, "canonical-domain")
                    .expect("canonical domain import"),
            },
            guest_contracts: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::JsQuickJs, "canonical-contracts")
                    .expect("canonical contracts import"),
            },
        };
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&canonical_contract_layout_ir(), &layout, &mut files)
            .expect("generate split JavaScript guest");
        layout
            .validate(Lang::JsQuickJs, &files.files)
            .expect("external contracts must be a valid binding dependency");
        let bindings: &GeneratedFile = files
            .files
            .iter()
            .find(|file| file.path == Path::new("guest/bindings.ts"))
            .expect("guest bindings");
        assert!(
            bindings
                .content
                .contains("import type { test_add_add } from 'canonical-contracts';"),
            "bindings must import the canonical contract alias: {}",
            bindings.content
        );
        assert!(
            bindings
                .content
                .contains("type TEST_ADDERProvider = { fn0: test_add_add };"),
            "provider surface must use the canonical function alias: {}",
            bindings.content
        );
        assert!(
            bindings
                .content
                .contains("test_adder_fn0_abi_wrapper(impl: TEST_ADDERProvider"),
            "wrapper must consume the canonical provider surface: {}",
            bindings.content
        );
        assert!(
            bindings
                .content
                .contains("hostHi: number) => TEST_ADDERProvider"),
            "factory must return the canonical provider surface: {}",
            bindings.content
        );
        assert!(
            bindings
                .references
                .contains(&OutputPartition::GuestContracts),
            "external canonical contracts must remain a semantic dependency"
        );

        let temp = tempdir().expect("temporary Deno project");
        let root: &Path = temp.path();
        fs::create_dir_all(root.join("guest")).expect("guest directory");
        fs::create_dir_all(root.join("canonical")).expect("canonical directory");
        fs::write(root.join("guest/bindings.ts"), &bindings.content).expect("guest bindings");
        fs::write(
            root.join("canonical/domain.ts"),
            "export interface AddArgs { readonly a: number; readonly b: number; }\n",
        )
        .expect("canonical domain");
        fs::write(
            root.join("deno.json"),
            r#"{"imports":{"canonical-domain":"./canonical/domain.ts","canonical-contracts":"./canonical/contracts.ts"}}"#,
        )
        .expect("Deno import map");
        fs::write(
            root.join("canonical/contracts.ts"),
            "import type { AddArgs } from \"canonical-domain\";\nexport type test_add_add = (args: AddArgs) => number;\n",
        )
        .expect("valid canonical contracts");
        let valid_check = Command::new("deno")
            .args([
                "check",
                "--quiet",
                "--config",
                "deno.json",
                "guest/bindings.ts",
            ])
            .current_dir(root)
            .output()
            .expect("run Deno typecheck");
        assert!(
            valid_check.status.success(),
            "valid canonical aliases must typecheck: {}",
            String::from_utf8_lossy(&valid_check.stderr)
        );
        fs::write(
            root.join("runtime.ts"),
            r#"import { TEST_ADDER_INTERFACE, setTestAdderFactory } from "./guest/bindings.ts";
setTestAdderFactory(() => ({ fn0: args => args.a + args.b }));
const slots = new Map<number, number>([[16, 12], [20, 30]]);
const bridge = {
    readU32(address: number): number { return slots.get(address) ?? 0; },
    writeU32(address: number, value: number): void { slots.set(address, value); },
};
const factory = TEST_ADDER_INTERFACE.factory;
if (factory === null) throw new Error("missing provider factory");
const provider = factory(bridge, 0, 0);
const result = TEST_ADDER_INTERFACE.functions[0](provider, 16, 32, 0, bridge);
if (result !== 0 || slots.get(32) !== 42) throw new Error("canonical provider dispatch failed");"#,
        )
        .expect("runtime E2E");
        let runtime = Command::new("deno")
            .args(["run", "--quiet", "--config", "deno.json", "runtime.ts"])
            .current_dir(root)
            .output()
            .expect("run Deno provider E2E");
        assert!(
            runtime.status.success(),
            "canonical provider must dispatch at runtime: {}",
            String::from_utf8_lossy(&runtime.stderr)
        );

        fs::write(
            root.join("canonical/contracts.ts"),
            "export type test_add_add = (args: string) => string;\n",
        )
        .expect("stale canonical contracts");
        let stale_check = Command::new("deno")
            .args([
                "check",
                "--quiet",
                "--config",
                "deno.json",
                "guest/bindings.ts",
            ])
            .current_dir(root)
            .output()
            .expect("run stale Deno typecheck");
        assert!(
            !stale_check.status.success(),
            "incompatible canonical aliases must fail typechecking"
        );
    }

    #[test]
    fn omitted_guest_contracts_emit_local_quickjs_provider_aliases() {
        let layout: OutputLayout = OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Inline,
            guest_contracts: OutputDestination::Omit,
        };
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&canonical_contract_layout_ir(), &layout, &mut files)
            .expect("generate omitted JavaScript guest contracts");
        layout
            .validate(Lang::JsQuickJs, &files.files)
            .expect("omitted contracts must be locally self-contained");
        let bindings: &GeneratedFile = files
            .files
            .iter()
            .find(|file| file.path == Path::new("guest/bindings.ts"))
            .expect("guest bindings");
        assert!(
            bindings
                .content
                .contains("export type test_add_add = (args: AddArgs) => number;"),
            "Omit must emit the one local contract alias used by the provider: {}",
            bindings.content
        );
        assert!(
            bindings
                .content
                .contains("type TEST_ADDERProvider = { fn0: test_add_add };"),
            "Omit must retain the same provider surface: {}",
            bindings.content
        );
        assert!(
            !bindings
                .references
                .contains(&OutputPartition::GuestContracts),
            "local aliases must not retain an omitted-partition reference"
        );
    }

    fn primitive_omit_ir() -> ValidatedIr {
        let contract = ResolvedContract {
            name: "test.increment".to_owned(),
            contract_id: guest_contract_id("test.increment", 1),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![ResolvedFunction {
                name: "increment".to_owned(),
                function_id: 0,
                params: vec![ResolvedParam {
                    name: "value".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                }],
                returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                docs: None,
                return_docs: None,
                langs: LanguageRules::default(),
                return_langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        };
        ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "primitive_omit".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                loader: "js-quickjs".to_owned(),
                file: ResolvedBundleFile::Single("plugin.js".to_owned()),
                plugins: vec![ResolvedPlugin {
                    name: "test_incrementer".to_owned(),
                    implements: vec!["test.increment@1.0".to_owned()],
                    optional: vec![],
                }],
                bundle_id: 0,
                dependencies: vec![],
                needs_reinit_on_dep_reload: false,
            }),
            langs: LanguageRules::default(),
        }
    }

    #[test]
    fn primitive_omitted_partitions_are_self_contained_for_quickjs_guest_and_internal() {
        let layout = OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Omit,
        };
        let ir = primitive_omit_ir();
        let generator = JsQuickjsGenerator;
        let mut guest_files = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &layout, &mut guest_files)
            .expect("generate primitive-only JavaScript guest");
        layout
            .validate(Lang::JsQuickJs, &guest_files.files)
            .expect("primitive guest must not retain omitted declaration dependencies");
        assert!(
            !guest_files.files.iter().any(|file| {
                matches!(
                    file.partition,
                    OutputPartition::DomainTypes | OutputPartition::GuestContracts
                )
            }),
            "primitive-only guest must emit neither declaration partition"
        );
        let guest_bindings = guest_files
            .files
            .iter()
            .find(|file| file.path == Path::new("guest/bindings.ts"))
            .expect("primitive guest bindings");
        assert!(
            !guest_bindings.content.contains("import type")
                && guest_bindings
                    .content
                    .contains("export type test_increment_increment = (value: number) => number;"),
            "primitive Omit bindings must carry a local type alias and no domain import: {}",
            guest_bindings.content
        );

        let mut internal_files = GeneratedFiles::default();
        generator
            .generate_internal_bundle(&ir, "primitive_omit", &layout, &mut internal_files)
            .expect("generate primitive-only JavaScript internal profile");
        layout
            .validate(Lang::JsQuickJs, &internal_files.files)
            .expect("primitive internal profile must not retain omitted declaration dependencies");
        assert!(
            !internal_files.files.iter().any(|file| {
                matches!(
                    file.partition,
                    OutputPartition::DomainTypes | OutputPartition::GuestContracts
                )
            }) && internal_files.files.iter().all(|file| {
                !file.references.iter().any(|reference| {
                    matches!(
                        reference,
                        OutputPartition::DomainTypes | OutputPartition::GuestContracts
                    )
                })
            }),
            "primitive internal profile must not emit or reference empty declaration partitions"
        );

        let temp = tempdir().expect("temporary primitive Deno project");
        let root = temp.path();
        fs::create_dir_all(root.join("guest")).expect("guest directory");
        fs::write(root.join("guest/bindings.ts"), &guest_bindings.content)
            .expect("primitive guest bindings");
        let typecheck = Command::new("deno")
            .args(["check", "--quiet", "guest/bindings.ts"])
            .current_dir(root)
            .output()
            .expect("typecheck primitive guest bindings");
        assert!(
            typecheck.status.success(),
            "primitive Omit bindings must typecheck: {}",
            String::from_utf8_lossy(&typecheck.stderr)
        );
        fs::write(
            root.join("runtime.ts"),
            r#"import { TEST_INCREMENTER_INTERFACE, setTestIncrementerFactory } from "./guest/bindings.ts";
setTestIncrementerFactory(() => ({ fn0: value => value + 1 }));
const slots = new Map<number, number>([[16, 41]]);
const bridge = {
    readU32(address: number): number { return slots.get(address) ?? 0; },
    writeU32(address: number, value: number): void { slots.set(address, value); },
};
const factory = TEST_INCREMENTER_INTERFACE.factory;
if (factory === null) throw new Error("missing provider factory");
const provider = factory(bridge, 0, 0);
const result = TEST_INCREMENTER_INTERFACE.functions[0](provider, 16, 32, 0, bridge);
if (result !== 0 || slots.get(32) !== 42) throw new Error("primitive provider dispatch failed");"#,
        )
        .expect("primitive runtime E2E");
        let runtime = Command::new("deno")
            .args(["run", "--quiet", "runtime.ts"])
            .current_dir(root)
            .output()
            .expect("run primitive provider E2E");
        assert!(
            runtime.status.success(),
            "primitive Omit provider must dispatch at runtime: {}",
            String::from_utf8_lossy(&runtime.stderr)
        );
    }

    // ─── Guest ABI wrapper marshalling (per-signature, C layout) ──────────────
    //
    // The wrappers were historically hardcoded for StringView→StringView; these
    // tests pin the per-signature marshalling for every other shape.

    fn wrapper_ir(types: Vec<ResolvedType>, enums: Vec<EnumDef>) -> ValidatedIr {
        ValidatedIr {
            types,
            enums,
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
            langs: LanguageRules::default(),
        }
    }

    fn wrapper_contract(functions: Vec<ResolvedFunction>) -> ResolvedContract {
        ResolvedContract {
            name: "test.shapes".to_owned(),
            contract_id: guest_contract_id("test.shapes", 1),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions,
            docs: None,
            langs: LanguageRules::default(),
        }
    }

    fn render_wrapper(contract: &ResolvedContract, ir: &ValidatedIr) -> String {
        let mut out: String = String::new();
        render_plugin_interface_quickjs(
            &mut out,
            JsPluginInterfaceConfig {
                plugin_name: "shapes_plugin",
                contract,
                ir,
                interface_var: "SHAPES_PLUGIN",
                set_factory_name: "setShapesPluginFactory",
                export_wrappers: false,
                use_contract_type_aliases: false,
            },
        )
        .expect("render_plugin_interface_quickjs");
        out
    }

    #[test]
    fn quickjs_guest_wrapper_scalar_pair_is_not_stringview_hardcoded() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "add_primitive".to_owned(),
            function_id: 0,
            params: vec![
                ResolvedParam {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedParam {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_a = polyplug.readU32(args_ptr);"),
            "first u32 param must be read at offset 0: {out}"
        );
        assert!(
            out.contains("var arg_b = polyplug.readU32(args_ptr + 4);"),
            "second u32 param must be read at offset 4: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr, result);"),
            "u32 return must be written with writeU32: {out}"
        );
        assert!(
            !out.contains("input_ptr_lo") && !out.contains("result.ptr_lo"),
            "scalar signature must not use the StringView hardcode: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_mixed_pack_uses_c_layout_alignment() {
        // (u32, StringView): the StringView sits at offset 8 in a repr(C)
        // pack (4 bytes of padding after the u32), NOT at offset 4.
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "log_with_count".to_owned(),
            function_id: 0,
            params: vec![
                ResolvedParam {
                    name: "count".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedParam {
                    name: "message".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_message = { ptr_lo: polyplug.readU32(args_ptr + 8), ptr_hi: polyplug.readU32(args_ptr + 12), len: polyplug.readU32(args_ptr + 16) };"),
            "StringView after u32 must be read at C-layout offset 8: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_stringview_return_arena_allocates() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "decode".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "input".to_owned(),
                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        // The wrapper takes the threaded arena_ptr + bridge and reads `polyplug`
        // from the bridge param (no global — Rule 12).
        assert!(
            out.contains(
                "(impl: any, args_ptr: number, out_ptr: number, arena_ptr: number, bridge: any): number"
            ),
            "wrapper must take the threaded arena_ptr + bridge: {out}"
        );
        assert!(
            out.contains("const polyplug = bridge;"),
            "wrapper must use the threaded bridge, not a global: {out}"
        );
        assert!(
            out.contains("var arg_input = { ptr_lo: polyplug.readU32(args_ptr), ptr_hi: polyplug.readU32(args_ptr + 4), len: polyplug.readU32(args_ptr + 8) };"),
            "single StringView param read at offset 0: {out}"
        );
        // The author returns a plain string; the wrapper arena-allocates it via the
        // threaded arena_ptr and writes the StringView back through out_ptr.
        assert!(
            out.contains("const _retBytes = _ppEncodeUtf8(result);")
                && out.contains(
                    "const _retBuf = polyplug.arenaAlloc(_retBytes.length > 0 ? _retBytes.length : 1, arena_ptr);"
                ),
            "StringView return must arena-allocate a plain string via the threaded arena: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr, _retBuf[0]);")
                && out.contains("polyplug.writeU32(out_ptr + 8, _retBytes.length);"),
            "StringView return written back through out_ptr: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr + 12, 0);"),
            "high half of the usize len must be zeroed: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_struct_param_reads_fields() {
        let types: Vec<ResolvedType> = vec![ResolvedType {
            name: "AddArgs".to_owned(),
            fields: vec![
                ResolvedField {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedField {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        }];
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "add".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "args".to_owned(),
                ty: ResolvedTypeRef::UserDefined("AddArgs".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_args = { a: polyplug.readU32(args_ptr), b: polyplug.readU32(args_ptr + 4) };"),
            "struct param must be read field-by-field at C offsets: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_nested_struct_param_and_return_recurse() {
        // A struct field that is itself a struct (depth > 1) must marshal through
        // the QuickJS guest wrapper, not be rejected. Layout:
        //   Inner { a: u32, b: u32 }            -> a@0, b@4 ; size 8, align 4
        //   Boxed { tag: u64, inner: Inner }    -> tag@0, inner@8 (8-aligned past
        //                                          the u64) ; size 16, align 8
        // proving both depth-2 recursion AND that the nested struct sits at the
        // C-layout offset its alignment demands.
        let types: Vec<ResolvedType> = vec![
            ResolvedType {
                name: "Inner".to_owned(),
                fields: vec![
                    ResolvedField {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                    ResolvedField {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            },
            ResolvedType {
                name: "Boxed".to_owned(),
                fields: vec![
                    ResolvedField {
                        name: "tag".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                    ResolvedField {
                        name: "inner".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("Inner".to_owned()),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            },
        ];
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "roundtrip".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "o".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Boxed".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: Some(ResolvedTypeRef::UserDefined("Boxed".to_owned())),
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let out: String = render_wrapper(&contract, &ir);

        // The depth guard is gone: no UnsupportedType / "nested struct" rejection.
        assert!(
            !out.contains("nested struct") && !out.contains("UnsupportedType"),
            "nested struct must not be rejected: {out}"
        );
        // READ (js_read_expr) of the nested param descends field-by-field, with the
        // inner struct read at the 8-aligned offset (8/12), not 4.
        assert!(
            out.contains("var arg_o = { tag: { lo: polyplug.readU32(args_ptr), hi: polyplug.readU32(args_ptr + 4) }, inner: { a: polyplug.readU32(args_ptr + 8), b: polyplug.readU32(args_ptr + 12) } };"),
            "nested struct param must read as a nested object literal at C offsets: {out}"
        );
        // WRITE (emit_js_write_value) of the nested return descends into the inner
        // struct's fields at the same 8-aligned offsets.
        assert!(
            out.contains("polyplug.writeU32(out_ptr + 8, result.inner.a);"),
            "nested return inner.a must write at offset 8: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr + 12, result.inner.b);"),
            "nested return inner.b must write at offset 12: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_f64_uses_float_bridge() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "scale".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "factor".to_owned(),
                ty: ResolvedTypeRef::Primitive(PrimitiveType::F64),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::F64)),
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_factor = polyplug.readF64(args_ptr);"),
            "f64 param must use readF64: {out}"
        );
        assert!(
            out.contains("polyplug.writeF64(out_ptr, result);"),
            "f64 return must use writeF64: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_enum_param_reads_repr_integer() {
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Info".to_owned(),
                value: "1".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        }];
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "set_level".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "level".to_owned(),
                ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], enums);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_level = polyplug.readU32(args_ptr);"),
            "u32-repr enum param must be read as the raw repr integer: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_void_void_emits_no_marshalling() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "reset".to_owned(),
            function_id: 0,
            params: vec![],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("impl.fn0();"),
            "void/void function must call the impl with no args: {out}"
        );
        assert!(
            !out.contains("readU32(args_ptr") && !out.contains("writeU32(out_ptr"),
            "void/void function must not touch args/out buffers: {out}"
        );
    }

    #[test]
    fn quickjs_caller_pack_uses_c_layout_alignment() {
        // Guest→host caller side of the same convention: (LogLevel u32,
        // StringView) packs to 24 bytes with the view at offset 8.
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Info".to_owned(),
                value: "1".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "log_with_level".to_owned(),
            function_id: 1,
            params: vec![
                ResolvedParam {
                    name: "level".to_owned(),
                    ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedParam {
                    name: "message".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut out, &func, &wrapper_ir(vec![], enums))
            .expect("args setup");
        assert!(
            out.contains("_callerAlloc(24)"),
            "pack must be 24 bytes (u32 + pad + 16-byte StringView): {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(argsPtr + 8, _messageDataBuf[0]);"),
            "StringView after u32 must be written at C-layout offset 8: {out}"
        );
    }

    // ─── Caller-side enum marshalling (repr-integer slots) ──────────────────────

    fn pixel_format_enums() -> Vec<EnumDef> {
        vec![EnumDef {
            name: "PixelFormat".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Rgba8".to_owned(),
                value: "1".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        }]
    }

    #[test]
    fn quickjs_caller_single_enum_param_writes_repr_integer() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_format".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "fmt".to_owned(),
                ty: ResolvedTypeRef::UserDefined("PixelFormat".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(
            &mut out,
            &func,
            &wrapper_ir(vec![], pixel_format_enums()),
        )
        .expect("args setup");
        assert!(
            out.contains("polyplug.writeU32(argsPtr, Number(fmt));"),
            "u32-repr enum param must be written into the arena slot: {out}"
        );
        assert!(
            out.contains("const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;"),
            "the slot's ADDRESS must be passed as argsPtr: {out}"
        );
    }

    #[test]
    fn quickjs_caller_enum_return_reads_repr_integer() {
        let returns: Option<ResolvedTypeRef> =
            Some(ResolvedTypeRef::UserDefined("PixelFormat".to_owned()));
        let mut out: String = String::new();
        emit_ts_guest_host_contract_readback(
            &mut out,
            returns.as_ref(),
            &wrapper_ir(vec![], pixel_format_enums()),
        )
        .expect("readback");
        assert!(
            out.contains("const result = polyplug.readU32(outPtr) as any;"),
            "u32-repr enum return must read back the repr integer: {out}"
        );
        assert!(
            !out.contains("{ lo:"),
            "enum return must NOT fall back to the {{lo, hi}} object shape: {out}"
        );
    }

    #[test]
    fn quickjs_caller_u64_enum_splits_words() {
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "BigFlags".to_owned(),
            repr: ReprType::U64,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "A".to_owned(),
                value: "1".to_owned(),
                docs: None,
                langs: LanguageRules::default(),
            }],
            docs: None,
            langs: LanguageRules::default(),
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_flags".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "flags".to_owned(),
                ty: ResolvedTypeRef::UserDefined("BigFlags".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        };
        let ir: ValidatedIr = wrapper_ir(vec![], enums);
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut out, &func, &ir).expect("args setup");
        assert!(
            out.contains("polyplug.writeU32(argsPtr, Number(flags) >>> 0);")
                && out.contains(
                    "polyplug.writeU32(argsPtr + 4, Math.floor(Number(flags) / 4294967296));"
                ),
            "u64-repr enum param must split into lo/hi words (writeU32 alone truncates): {out}"
        );

        let returns: Option<ResolvedTypeRef> =
            Some(ResolvedTypeRef::UserDefined("BigFlags".to_owned()));
        let mut ret: String = String::new();
        emit_ts_guest_host_contract_readback(&mut ret, returns.as_ref(), &ir).expect("readback");
        assert!(
            ret.contains(
                "(polyplug.readU32(outPtr) + polyplug.readU32(outPtr + 4) * 4294967296) as any"
            ),
            "u64-repr enum return must combine lo/hi words: {ret}"
        );
    }

    #[test]
    fn quickjs_caller_struct_param_and_return_marshal_field_by_field() {
        // Struct-by-value param/return on the guest→host caller path must marshal
        // field-by-field (NOT Number(structObj) = NaN into a 4-byte slot).
        // Pair { a: u32, b: u32 } -> a@0, b@4 ; size 8, align 4.
        let types: Vec<ResolvedType> = vec![ResolvedType {
            name: "Pair".to_owned(),
            fields: vec![
                ResolvedField {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedField {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        }];
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let func: ResolvedFunction = ResolvedFunction {
            name: "compute".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "args".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        };

        // PARAM: allocate the struct's C size and pack each field — no NaN.
        let mut args: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut args, &func, &ir).expect("args setup");
        assert!(
            args.contains("_callerAlloc(8)"),
            "struct param slot must be the C-layout size (8), not a 4-byte slot: {args}"
        );
        assert!(
            args.contains("polyplug.writeU32(argsPtr, args.a);")
                && args.contains("polyplug.writeU32(argsPtr + 4, args.b);"),
            "struct param must be packed field-by-field at C offsets: {args}"
        );
        assert!(
            !args.contains("Number(args)"),
            "struct param must NOT degrade to Number(structObj) = NaN: {args}"
        );

        // RETURN out-setup: allocate the exact struct size (not an 8-byte pointer slot).
        let mut outs: String = String::new();
        emit_ts_guest_host_contract_out_setup(&mut outs, &func.returns, &ir).expect("out setup");
        assert!(
            outs.contains("_callerAlloc(8)"),
            "struct return out slot must be the C-layout size: {outs}"
        );

        // RETURN readback: read the struct field-by-field into an object literal.
        let mut rb: String = String::new();
        emit_ts_guest_host_contract_readback(&mut rb, func.returns.as_ref(), &ir)
            .expect("readback");
        assert!(
            rb.contains(
                "const result = { a: polyplug.readU32(outPtr), b: polyplug.readU32(outPtr + 4) };"
            ),
            "struct return must be read field-by-field, not as a {{lo,hi}} pointer: {rb}"
        );
    }

    #[test]
    fn quickjs_caller_struct_with_stringview_field_arena_allocates() {
        // A struct param whose field is a StringView must arena-allocate the
        // string bytes (matching the Deno caller) — no per-language limitation.
        // Holder { name: StringView, code: u32 } -> name@0 (16,8), code@16 ; size 24.
        let types: Vec<ResolvedType> = vec![ResolvedType {
            name: "Holder".to_owned(),
            fields: vec![
                ResolvedField {
                    name: "name".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedField {
                    name: "code".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            docs: None,
            langs: LanguageRules::default(),
        }];
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let func: ResolvedFunction = ResolvedFunction {
            name: "store".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "args".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Holder".to_owned()),
                docs: None,
                langs: LanguageRules::default(),
            }],
            returns: None,
            docs: None,
            return_docs: None,
            langs: LanguageRules::default(),
            return_langs: LanguageRules::default(),
        };
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut out, &func, &ir).expect("args setup");
        assert!(
            out.contains("_callerAlloc(24)"),
            "Holder slot must be 24 bytes (16-byte StringView + u32, 8-aligned): {out}"
        );
        assert!(
            out.contains("const _sv0Bytes = _ppEncodeUtf8(args.name);"),
            "StringView struct field must encode from the field expression (TextEncoder-free): {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(argsPtr + 16, args.code);"),
            "scalar field after the 16-byte StringView must write at offset 16: {out}"
        );
    }

    // ─── Deno host caller — full-shape marshalling ──────────────────────────────

    /// Build an IR exercising every Deno host-caller marshalling shape: a
    /// multi-param function (u32 + u64 + enum + StringView in, StringView out),
    /// a struct parameter, a struct return, and an enum return.
    fn deno_shapes_ir() -> ValidatedIr {
        ValidatedIr {
            types: vec![ResolvedType {
                name: "Pair".to_owned(),
                fields: vec![
                    ResolvedField {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                    ResolvedField {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::I64),
                        docs: None,
                        langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            }],
            enums: vec![EnumDef {
                name: "Color".to_owned(),
                repr: ReprType::U32,
                bitflag: false,
                variants: vec![EnumVariant {
                    name: "Red".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                    langs: LanguageRules::default(),
                }],
                docs: None,
                langs: LanguageRules::default(),
            }],
            contracts: vec![ResolvedContract {
                name: "test.shapes".to_owned(),
                contract_id: 0x0011_2233_4455_6677_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![
                    ResolvedFunction {
                        name: "mix".to_owned(),
                        function_id: 0,
                        params: vec![
                            ResolvedParam {
                                name: "n".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                            ResolvedParam {
                                name: "big".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                            ResolvedParam {
                                name: "c".to_owned(),
                                ty: ResolvedTypeRef::UserDefined("Color".to_owned()),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                            ResolvedParam {
                                name: "s".to_owned(),
                                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                        ],
                        returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "take_struct".to_owned(),
                        function_id: 1,
                        params: vec![ResolvedParam {
                            name: "p".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
                            docs: None,
                            langs: LanguageRules::default(),
                        }],
                        returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "get_struct".to_owned(),
                        function_id: 2,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "get_color".to_owned(),
                        function_id: 3,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Color".to_owned())),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            }],
            host_contracts: vec![],
            bundle: None,
            langs: LanguageRules::default(),
        }
    }

    #[test]
    fn deno_host_caller_marshals_all_shapes() {
        let ir: ValidatedIr = deno_shapes_ir();
        let out: String = generate_callers_ts(&ir).expect("generate Deno callers");

        // The old "shape not supported" stub must be gone for every shape.
        assert!(
            !out.contains("shape not supported"),
            "no caller shape may emit the unsupported stub: {out}"
        );

        // Ergonomic, import-free TS signatures.
        assert!(
            out.contains("mix(n: number, big: bigint, c: number, s: string): string {"),
            "mix signature must map each param to its ergonomic Deno type: {out}"
        );
        assert!(
            out.contains("take_struct(p: { a: number; b: bigint }): number {"),
            "struct param must be an inline object type: {out}"
        );
        assert!(
            out.contains("get_struct(): { a: number; b: bigint } {"),
            "struct return must be an inline object type: {out}"
        );
        assert!(
            out.contains("get_color(): number {"),
            "enum return must be `number`: {out}"
        );

        // `mix` args struct: u32@0, u64@8 (8-aligned), Color u32@16, StringView@24
        // → 40-byte buffer.
        assert!(
            out.contains("const argsBuf = new Uint8Array(40);"),
            "mix args must pack to 40 bytes under C layout: {out}"
        );
        assert!(
            out.contains("argsDv.setBigUint64(8, BigInt(big), true);"),
            "u64 param must be written at offset 8 as bigint: {out}"
        );
        // Enum param written UNSIGNED at its repr width (the #34 item-3 fix).
        assert!(
            out.contains("argsDv.setUint32(16, Number(c) >>> 0, true);"),
            "u32-repr enum param must be written UNSIGNED at offset 16: {out}"
        );
        // StringView param: host-allocated, length+ptr written, tracked for free.
        assert!(
            out.contains("rt.alloc(_sv0Alloc, 1)")
                && out.contains("_allocs.push([_sv0Ptr, _sv0Alloc]);"),
            "StringView param must be host-allocated and tracked for release: {out}"
        );
        assert!(
            out.contains(
                "argsDv.setBigUint64(24, BigInt(Deno.UnsafePointer.value(_sv0Ptr)), true);"
            ),
            "StringView param ptr must be written at C-layout offset 24: {out}"
        );
        // All argument payloads are freed after dispatch.
        assert!(
            out.contains("for (const [_p, _s] of _allocs) { rt.free(_p, _s, 1); }"),
            "argument payloads must be freed after dispatch: {out}"
        );
        // StringView return: decoded then freed.
        assert!(
            out.contains("_decoder.decode(") && out.contains("rt.free(_ptr, _l, 1);"),
            "returned StringView must be decoded and freed: {out}"
        );

        // Struct return read field-by-field, UNSIGNED/signed at exact widths.
        assert!(
            out.contains("const _r1 = outDv.getUint32(0, true);"),
            "struct return u32 field must read at offset 0: {out}"
        );
        assert!(
            out.contains("const _r2 = outDv.getBigInt64(8, true);"),
            "struct return i64 field must read as signed bigint at offset 8: {out}"
        );
        assert!(
            out.contains("const _r0 = { a: _r1, b: _r2 };"),
            "struct return must be assembled into an object literal: {out}"
        );

        // Enum return read UNSIGNED at repr width — never the {lo,hi} object.
        assert!(
            out.contains("const _r0 = outDv.getUint32(0, true);"),
            "u32-repr enum return must read the UNSIGNED repr integer: {out}"
        );
        assert!(
            !out.contains("getBigInt64") || !out.contains("Number(outDv.getBigInt64"),
            "enum returns must not sign-extend: {out}"
        );
    }

    #[test]
    fn deno_host_caller_marshals_nested_struct() {
        // A struct field that is itself a struct (depth > 1) must marshal through
        // the Deno host caller, not be rejected. Same layout as the QuickJS guest
        // nested test:
        //   Inner { a: u32, b: u32 }          -> a@0, b@4 ; size 8, align 4
        //   Boxed { tag: u64, inner: Inner }  -> tag@0, inner@8 ; size 16, align 8
        let ir: ValidatedIr = ValidatedIr {
            types: vec![
                ResolvedType {
                    name: "Inner".to_owned(),
                    fields: vec![
                        ResolvedField {
                            name: "a".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                        ResolvedField {
                            name: "b".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                    ],
                    docs: None,
                    langs: LanguageRules::default(),
                },
                ResolvedType {
                    name: "Boxed".to_owned(),
                    fields: vec![
                        ResolvedField {
                            name: "tag".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                        ResolvedField {
                            name: "inner".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Inner".to_owned()),
                            docs: None,
                            langs: LanguageRules::default(),
                        },
                    ],
                    docs: None,
                    langs: LanguageRules::default(),
                },
            ],
            enums: vec![],
            contracts: vec![ResolvedContract {
                name: "test.nested".to_owned(),
                contract_id: 0x0011_2233_4455_6677_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![
                    ResolvedFunction {
                        name: "take_box".to_owned(),
                        function_id: 0,
                        params: vec![ResolvedParam {
                            name: "o".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Boxed".to_owned()),
                            docs: None,
                            langs: LanguageRules::default(),
                        }],
                        returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "get_box".to_owned(),
                        function_id: 1,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Boxed".to_owned())),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            }],
            host_contracts: vec![],
            bundle: None,
            langs: LanguageRules::default(),
        };
        let out: String = generate_callers_ts(&ir).expect("generate Deno callers");

        // The depth guard is gone: nothing rejects the nested struct.
        assert!(
            !out.contains("shape not supported")
                && !out.contains("UnsupportedType")
                && !out.contains("nested struct"),
            "nested struct must not be rejected by the Deno caller: {out}"
        );
        // Nested inline TS types (deno_caller_ts_type recurses).
        assert!(
            out.contains("take_box(o: { tag: bigint; inner: { a: number; b: number } }): number {"),
            "nested struct param must be a nested inline object type: {out}"
        );
        assert!(
            out.contains("get_box(): { tag: bigint; inner: { a: number; b: number } } {"),
            "nested struct return must be a nested inline object type: {out}"
        );
        // WRITE (emit_deno_write_value) descends into the inner struct at the
        // 8-aligned C offsets (8/12), past the u64 tag at 0.
        assert!(
            out.contains("argsDv.setBigUint64(0, BigInt(o.tag), true);"),
            "u64 tag must be written at offset 0: {out}"
        );
        assert!(
            out.contains("argsDv.setUint32(8, Number(o.inner.a) >>> 0, true);"),
            "nested inner.a must be written at offset 8: {out}"
        );
        assert!(
            out.contains("argsDv.setUint32(12, Number(o.inner.b) >>> 0, true);"),
            "nested inner.b must be written at offset 12: {out}"
        );
        // READ (emit_deno_read_local) assembles the nested return object from the
        // inner fields read at offsets 8/12.
        assert!(
            out.contains("const _r2 = { a: _r3, b: _r4 };")
                && out.contains("const _r0 = { tag: _r1, inner: _r2 };"),
            "nested struct return must assemble a nested object literal: {out}"
        );
        assert!(
            out.contains("outDv.getUint32(8, true)") && out.contains("outDv.getUint32(12, true)"),
            "nested inner fields must be read at offsets 8/12: {out}"
        );
    }

    // ─── Deno host PROVIDER — full-shape marshalling ────────────────────────────

    /// Build an IR with a HOST contract exercising every provider marshalling
    /// shape: a multi-param method (u32 + u64 + enum + StringView in, StringView
    /// out), a struct parameter with a scalar return, a Buffer parameter, and a
    /// struct return. Proves the provider has no type-support limitation.
    fn deno_host_provider_shapes_ir() -> ValidatedIr {
        let base: ValidatedIr = deno_shapes_ir();
        ValidatedIr {
            types: base.types,
            enums: base.enums,
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.shapes".to_owned(),
                contract_id: 0x0011_2233_4455_6677_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![
                    ResolvedFunction {
                        name: "mix".to_owned(),
                        function_id: 0,
                        params: vec![
                            ResolvedParam {
                                name: "n".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                            ResolvedParam {
                                name: "big".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                            ResolvedParam {
                                name: "c".to_owned(),
                                ty: ResolvedTypeRef::UserDefined("Color".to_owned()),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                            ResolvedParam {
                                name: "s".to_owned(),
                                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                                docs: None,
                                langs: LanguageRules::default(),
                            },
                        ],
                        returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "take_struct".to_owned(),
                        function_id: 1,
                        params: vec![ResolvedParam {
                            name: "p".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
                            docs: None,
                            langs: LanguageRules::default(),
                        }],
                        returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "take_buffer".to_owned(),
                        function_id: 2,
                        params: vec![ResolvedParam {
                            name: "b".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::Buffer),
                            docs: None,
                            langs: LanguageRules::default(),
                        }],
                        returns: None,
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                    ResolvedFunction {
                        name: "get_struct".to_owned(),
                        function_id: 3,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
                        docs: None,
                        return_docs: None,
                        langs: LanguageRules::default(),
                        return_langs: LanguageRules::default(),
                    },
                ],
                docs: None,
                langs: LanguageRules::default(),
            }],
            bundle: None,
            langs: LanguageRules::default(),
        }
    }

    #[test]
    fn deno_host_provider_marshals_all_shapes() {
        let ir: ValidatedIr = deno_host_provider_shapes_ir();
        let out: String =
            generate_js_host_interface_factories_ts(&ir).expect("generate Deno host provider");

        // Every shape generates — no type is rejected as unsupported.
        assert!(
            !out.contains("UnsupportedType") && !out.contains("unsupported"),
            "no provider shape may be unsupported: {out}"
        );

        // Multi-param method: u32@0, u64@8, Color(u32)@16, StringView@24 (ptr+len)
        // → 40-byte args buffer; all four reach the impl.
        assert!(
            out.contains("const _argsDv = new DataView(new Deno.UnsafePointerView(argsPtr!).getArrayBuffer(40));"),
            "mix args must read from a 40-byte C-layout buffer: {out}"
        );
        assert!(
            out.contains("_argsDv.getBigUint64(8, true)"),
            "u64 arg must be read at offset 8: {out}"
        );
        assert!(
            out.contains("_argsDv.getUint32(16, true)"),
            "u32-repr enum arg must be read UNSIGNED at offset 16: {out}"
        );
        assert!(
            out.contains("impl.Mix(_r0, _r1, _r2, _r3);"),
            "all four args must reach the impl: {out}"
        );

        // StringView RETURN: host-allocated via rt.alloc (the guest frees it later).
        assert!(
            out.contains("rt.alloc(") && out.contains("_outDv.setBigUint64(0,"),
            "StringView return must host-allocate its payload and write the slot: {out}"
        );

        // Struct param read into an object; struct return written field-by-field.
        assert!(
            out.contains("const _r0 = { a:") || out.contains("a: _r"),
            "struct param must be read into an object literal: {out}"
        );
        assert!(
            out.contains("impl.GetStruct()") && out.contains("_outDv.setUint32(0,"),
            "struct return must write its scalar field: {out}"
        );

        // Buffer param read into a Uint8Array copy.
        assert!(
            out.contains("new Uint8Array(0)") && out.contains("impl.TakeBuffer(_r0);"),
            "Buffer param must be read into a Uint8Array and passed to the impl: {out}"
        );

        // CRITICAL ownership: the provider BORROWS its args — it must never free a
        // caller-owned arg payload (that would be a use-after-free). The only host
        // allocation is for the StringView RETURN; there is no rt.free anywhere.
        assert!(
            !out.contains("rt.free"),
            "provider must NOT free caller-owned arg payloads: {out}"
        );
    }
    #[test]
    fn external_guest_omits_legacy_manifest_helper() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "js.external".to_owned(),
                version: Version {
                    major: 1,
                    minor: 2,
                    patch: 3,
                },
                loader: "js-quickjs".to_owned(),
                file: ResolvedBundleFile::Single("plugin.js".to_owned()),
                plugins: vec![],
                bundle_id: 0x1234_5678_9ABC_DEF0,
                dependencies: vec![],
                needs_reinit_on_dep_reload: false,
            }),
            langs: LanguageRules::default(),
        };
        let init: String = generate_init_ts(&ir);
        let index: String = generate_index_ts(&ir);

        assert!(
            !init.contains("POLYPLUG_MANIFEST"),
            "external guest init must not emit internal-plugin manifest helpers: {init}"
        );
        assert!(
            !index.contains("POLYPLUG_MANIFEST"),
            "external guest index must not re-export internal-plugin manifest helpers: {index}"
        );
    }
}
