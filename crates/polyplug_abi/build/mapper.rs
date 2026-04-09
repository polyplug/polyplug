//! Mapper module — converts ABI types to polyplug_codegen data types.
//!
//! This module provides functions to map `AbiType` (extracted from `src/lib.rs`)
//! to `Item` (used by language generators). It also provides hash function
//! generation for all SDKs.

use crate::types::{
    AbiConst, AbiEnum, AbiField, AbiFunction, AbiStruct, AbiType, AbiUnion, AbiUnionVariant,
    AbiVariant,
};
use polyplug_codegen::data::{
    ConstInfo, EnumInfo, EnumVariant, FieldInfo, FunctionInfo, Item, ParamInfo, StructInfo,
    UnionInfo, UnionVariant,
};

/// Map an ABI type to a codegen Item.
pub fn map_abi_to_codegen(abi: &AbiType) -> Item {
    match abi {
        AbiType::Struct(s) => Item::Struct(map_struct(s)),
        AbiType::Enum(e) => Item::Enum(map_enum(e)),
        AbiType::Union(u) => Item::Union(map_union(u)),
        AbiType::Function(f) => Item::Function(map_function(f)),
        AbiType::Const(c) => Item::Const(map_const(c)),
    }
}

/// Map an AbiStruct to StructInfo.
fn map_struct(abi: &AbiStruct) -> StructInfo {
    let mut attributes: Vec<String> = Vec::new();
    if abi.repr_c {
        attributes.push(String::from("repr(C)"));
    }

    StructInfo {
        name: abi.name.clone(),
        fields: abi.fields.iter().map(map_field).collect(),
        doc: abi.doc.clone(),
        attributes,
    }
}

/// Map an AbiField to FieldInfo.
fn map_field(abi: &AbiField) -> FieldInfo {
    FieldInfo {
        name: abi.name.clone(),
        rust_type: abi.rust_type.clone(),
        doc: abi.doc.clone(),
    }
}

/// Map an AbiEnum to EnumInfo.
fn map_enum(abi: &AbiEnum) -> EnumInfo {
    EnumInfo {
        name: abi.name.clone(),
        repr: abi.repr.clone(),
        variants: abi.variants.iter().map(map_variant).collect(),
        doc: abi.doc.clone(),
    }
}

/// Map an AbiVariant to EnumVariant.
fn map_variant(abi: &AbiVariant) -> EnumVariant {
    EnumVariant {
        name: abi.name.clone(),
        value: abi.value,
        doc: abi.doc.clone(),
    }
}

/// Map an AbiUnion to UnionInfo.
fn map_union(abi: &AbiUnion) -> UnionInfo {
    UnionInfo {
        name: abi.name.clone(),
        variants: abi.variants.iter().map(map_union_variant).collect(),
        doc: abi.doc.clone(),
    }
}

/// Map an AbiUnionVariant to UnionVariant.
fn map_union_variant(abi: &AbiUnionVariant) -> UnionVariant {
    UnionVariant {
        name: abi.name.clone(),
        type_name: abi.rust_type.clone(),
    }
}

/// Map an AbiFunction to FunctionInfo.
fn map_function(abi: &AbiFunction) -> FunctionInfo {
    FunctionInfo {
        name: abi.name.clone(),
        params: abi
            .params
            .iter()
            .map(|f: &AbiField| ParamInfo {
                name: f.name.clone(),
                rust_type: f.rust_type.clone(),
                doc: f.doc.clone(),
            })
            .collect(),
        return_type: abi.return_type.clone(),
        is_constexpr: false,
        doc: abi.doc.clone(),
    }
}

/// Map an AbiConst to ConstInfo.
fn map_const(abi: &AbiConst) -> ConstInfo {
    ConstInfo {
        name: abi.name.clone(),
        rust_type: abi.rust_type.clone(),
        value: abi.value.clone(),
        doc: abi.doc.clone(),
    }
}

/// Create hash function items for all languages.
///
/// These functions are added to every SDK so that all languages have
/// access to FNV-1a hashing for contract IDs, bundle IDs, etc.
pub fn create_hash_functions() -> Vec<Item> {
    vec![
        // fnv1a_64(data: &[u8]) -> u64
        Item::Function(FunctionInfo {
            name: String::from("fnv1a_64"),
            params: vec![ParamInfo {
                name: String::from("data"),
                rust_type: String::from("&[u8]"),
                doc: Some(String::from("Input byte slice.")),
            }],
            return_type: Some(String::from("u64")),
            is_constexpr: true,
            doc: Some(String::from(
                "Compute FNV-1a 64-bit hash of a byte sequence.",
            )),
        }),
        // contract_id(name: &str, major: u32) -> u64
        Item::Function(FunctionInfo {
            name: String::from("contract_id"),
            params: vec![
                ParamInfo {
                    name: String::from("name"),
                    rust_type: String::from("&str"),
                    doc: Some(String::from("Contract name.")),
                },
                ParamInfo {
                    name: String::from("major"),
                    rust_type: String::from("u32"),
                    doc: Some(String::from("Major version.")),
                },
            ],
            return_type: Some(String::from("u64")),
            is_constexpr: true,
            doc: Some(String::from(
                "Compute the contract ID for \"name@major_version\" using FNV-1a 64-bit.",
            )),
        }),
        // bundle_id(name: &str) -> u64
        Item::Function(FunctionInfo {
            name: String::from("bundle_id"),
            params: vec![ParamInfo {
                name: String::from("name"),
                rust_type: String::from("&str"),
                doc: Some(String::from("Bundle name.")),
            }],
            return_type: Some(String::from("u64")),
            is_constexpr: true,
            doc: Some(String::from(
                "Compute a bundle ID from its name using FNV-1a 64-bit hash.",
            )),
        }),
        // host_contract_id(name: &str, major: u32) -> u64
        Item::Function(FunctionInfo {
            name: String::from("host_contract_id"),
            params: vec![
                ParamInfo {
                    name: String::from("name"),
                    rust_type: String::from("&str"),
                    doc: Some(String::from("Host contract name.")),
                },
                ParamInfo {
                    name: String::from("major"),
                    rust_type: String::from("u32"),
                    doc: Some(String::from("Major version.")),
                },
            ],
            return_type: Some(String::from("u64")),
            is_constexpr: true,
            doc: Some(String::from(
                "Calculate host contract ID from name and major version.",
            )),
        }),
        // plugin_contract_id(name: &str, major: u32) -> u64
        Item::Function(FunctionInfo {
            name: String::from("plugin_contract_id"),
            params: vec![
                ParamInfo {
                    name: String::from("name"),
                    rust_type: String::from("&str"),
                    doc: Some(String::from("Plugin contract name.")),
                },
                ParamInfo {
                    name: String::from("major"),
                    rust_type: String::from("u32"),
                    doc: Some(String::from("Major version.")),
                },
            ],
            return_type: Some(String::from("u64")),
            is_constexpr: true,
            doc: Some(String::from(
                "Calculate plugin contract ID from name and major version.",
            )),
        }),
    ]
}

/// Map all ABI types to codegen Items.
///
/// This is a convenience function that maps all types in an `AbiTypes`
/// collection to a vector of `Item`.
pub fn map_all_abi_types(types: &[AbiType]) -> Vec<Item> {
    types.iter().map(map_abi_to_codegen).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_struct() {
        let abi_struct: AbiStruct = AbiStruct {
            name: String::from("StringView"),
            fields: vec![
                AbiField {
                    name: String::from("ptr"),
                    rust_type: String::from("*const u8"),
                    doc: Some(String::from("UTF-8 bytes.")),
                },
                AbiField {
                    name: String::from("len"),
                    rust_type: String::from("usize"),
                    doc: Some(String::from("Byte count.")),
                },
            ],
            doc: Some(String::from("Non-owning UTF-8 string view.")),
            repr_c: true,
        };

        let item: Item = map_abi_to_codegen(&AbiType::Struct(abi_struct));

        match item {
            Item::Struct(info) => {
                assert_eq!(info.name, "StringView");
                assert_eq!(info.fields.len(), 2);
                assert_eq!(info.fields[0].name, "ptr");
                assert_eq!(info.fields[0].rust_type, "*const u8");
                assert_eq!(info.fields[1].name, "len");
                assert_eq!(info.fields[1].rust_type, "usize");
                assert_eq!(
                    info.doc,
                    Some(String::from("Non-owning UTF-8 string view."))
                );
                assert_eq!(info.attributes, vec!["repr(C)"]);
            }
            _ => panic!("Expected Struct item"),
        }
    }

    #[test]
    fn test_map_enum() {
        let abi_enum: AbiEnum = AbiEnum {
            name: String::from("DispatchType"),
            repr: String::from("u32"),
            variants: vec![
                AbiVariant {
                    name: String::from("Native"),
                    value: Some(0),
                    doc: Some(String::from("Native dispatch.")),
                },
                AbiVariant {
                    name: String::from("VirtualMachine"),
                    value: Some(1),
                    doc: Some(String::from("VM dispatch.")),
                },
            ],
            doc: Some(String::from("Dispatch mechanism type.")),
        };

        let item: Item = map_abi_to_codegen(&AbiType::Enum(abi_enum));

        match item {
            Item::Enum(info) => {
                assert_eq!(info.name, "DispatchType");
                assert_eq!(info.repr, "u32");
                assert_eq!(info.variants.len(), 2);
                assert_eq!(info.variants[0].name, "Native");
                assert_eq!(info.variants[0].value, Some(0));
                assert_eq!(info.variants[1].name, "VirtualMachine");
                assert_eq!(info.variants[1].value, Some(1));
            }
            _ => panic!("Expected Enum item"),
        }
    }

    #[test]
    fn test_map_union() {
        let abi_union: AbiUnion = AbiUnion {
            name: String::from("PluginDispatch"),
            variants: vec![
                AbiUnionVariant {
                    name: String::from("native"),
                    rust_type: String::from("NativeDispatch"),
                    doc: None,
                },
                AbiUnionVariant {
                    name: String::from("vm"),
                    rust_type: String::from("VmDispatch"),
                    doc: None,
                },
            ],
            doc: Some(String::from("Union of dispatch mechanisms.")),
        };

        let item: Item = map_abi_to_codegen(&AbiType::Union(abi_union));

        match item {
            Item::Union(info) => {
                assert_eq!(info.name, "PluginDispatch");
                assert_eq!(info.variants.len(), 2);
                assert_eq!(info.variants[0].name, "native");
                assert_eq!(info.variants[0].type_name, "NativeDispatch");
                assert_eq!(info.variants[1].name, "vm");
                assert_eq!(info.variants[1].type_name, "VmDispatch");
            }
            _ => panic!("Expected Union item"),
        }
    }

    #[test]
    fn test_map_function() {
        let abi_func: AbiFunction = AbiFunction {
            name: String::from("contract_id"),
            params: vec![
                AbiField {
                    name: String::from("name"),
                    rust_type: String::from("&str"),
                    doc: None,
                },
                AbiField {
                    name: String::from("major"),
                    rust_type: String::from("u32"),
                    doc: None,
                },
            ],
            return_type: Some(String::from("u64")),
            doc: Some(String::from("Compute contract ID.")),
        };

        let item: Item = map_abi_to_codegen(&AbiType::Function(abi_func));

        match item {
            Item::Function(info) => {
                assert_eq!(info.name, "contract_id");
                assert_eq!(info.params.len(), 2);
                assert_eq!(info.params[0].name, "name");
                assert_eq!(info.params[0].rust_type, "&str");
                assert_eq!(info.params[1].name, "major");
                assert_eq!(info.params[1].rust_type, "u32");
                assert_eq!(info.return_type, Some(String::from("u64")));
            }
            _ => panic!("Expected Function item"),
        }
    }

    #[test]
    fn test_map_const() {
        let abi_const: AbiConst = AbiConst {
            name: String::from("POLYPLUG_ABI_VERSION"),
            rust_type: String::from("u32"),
            value: String::from("1"),
            doc: Some(String::from("ABI version sentinel.")),
        };

        let item: Item = map_abi_to_codegen(&AbiType::Const(abi_const));

        match item {
            Item::Const(info) => {
                assert_eq!(info.name, "POLYPLUG_ABI_VERSION");
                assert_eq!(info.rust_type, "u32");
                assert_eq!(info.value, "1");
                assert_eq!(info.doc, Some(String::from("ABI version sentinel.")));
            }
            _ => panic!("Expected Const item"),
        }
    }

    #[test]
    fn test_create_hash_functions() {
        let items: Vec<Item> = create_hash_functions();

        assert_eq!(items.len(), 5);

        // Verify fnv1a_64
        match &items[0] {
            Item::Function(f) => {
                assert_eq!(f.name, "fnv1a_64");
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name, "data");
                assert_eq!(f.return_type, Some(String::from("u64")));
            }
            _ => panic!("Expected Function item"),
        }

        // Verify contract_id
        match &items[1] {
            Item::Function(f) => {
                assert_eq!(f.name, "contract_id");
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0].name, "name");
                assert_eq!(f.params[1].name, "major");
            }
            _ => panic!("Expected Function item"),
        }

        // Verify bundle_id
        match &items[2] {
            Item::Function(f) => {
                assert_eq!(f.name, "bundle_id");
                assert_eq!(f.params.len(), 1);
            }
            _ => panic!("Expected Function item"),
        }

        // Verify host_contract_id
        match &items[3] {
            Item::Function(f) => {
                assert_eq!(f.name, "host_contract_id");
                assert_eq!(f.params.len(), 2);
            }
            _ => panic!("Expected Function item"),
        }

        // Verify plugin_contract_id
        match &items[4] {
            Item::Function(f) => {
                assert_eq!(f.name, "plugin_contract_id");
                assert_eq!(f.params.len(), 2);
            }
            _ => panic!("Expected Function item"),
        }
    }

    #[test]
    fn test_map_all_abi_types() {
        let types: Vec<AbiType> = vec![
            AbiType::Const(AbiConst {
                name: String::from("POLYPLUG_ABI_VERSION"),
                rust_type: String::from("u32"),
                value: String::from("1"),
                doc: None,
            }),
            AbiType::Struct(AbiStruct {
                name: String::from("StringView"),
                fields: vec![],
                doc: None,
                repr_c: true,
            }),
        ];

        let items: Vec<Item> = map_all_abi_types(&types);

        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], Item::Const(_)));
        assert!(matches!(&items[1], Item::Struct(_)));
    }
}
