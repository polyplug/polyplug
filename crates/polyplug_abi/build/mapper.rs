//! Mapper module — converts ABI types to polyplug_codegen data types.
//!
//! This module provides functions to map `AbiType` (extracted from the module tree)
//! to `Item` (used by language generators).

use crate::types::{
    AbiConst, AbiEnum, AbiField, AbiStruct, AbiType, AbiUnion, AbiUnionVariant, AbiVariant,
};
use polyplug_codegen::data::{
    ConstInfo, EnumInfo, EnumVariant, FieldInfo, Item, StructInfo, UnionInfo, UnionVariant,
};

/// Map an ABI type to a codegen Item.
pub fn map_abi_to_codegen(abi: &AbiType) -> Item {
    match abi {
        AbiType::Struct(s) => Item::Struct(map_struct(s)),
        AbiType::Enum(e) => Item::Enum(map_enum(e)),
        AbiType::Union(u) => Item::Union(map_union(u)),
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

/// Map an AbiConst to ConstInfo.
fn map_const(abi: &AbiConst) -> ConstInfo {
    ConstInfo {
        name: abi.name.clone(),
        rust_type: abi.rust_type.clone(),
        value: abi.value.clone(),
        doc: abi.doc.clone(),
    }
}

/// Map all ABI types to codegen Items.
///
/// This is a convenience function that maps all types in an `AbiTypes`
/// collection to a vector of `Item`.
pub fn map_all_abi_types(types: &[AbiType]) -> Vec<Item> {
    types.iter().map(map_abi_to_codegen).collect()
}
