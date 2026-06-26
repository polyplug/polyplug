//! ABI-specific types for type extraction from syn AST.
//!
//! These types represent the extracted ABI information from the module tree.
//! They are used by the extractor module and converted to
//! `polyplug_codegen::data::AbiItems` for code generation.

#![allow(dead_code)]

/// A single ABI type extracted from the source.
#[derive(Debug, Clone)]
pub enum AbiType {
    /// A struct definition (e.g., StringView, Buffer).
    Struct(AbiStruct),
    /// An enum definition (e.g., DispatchType).
    Enum(AbiEnum),
    /// A union definition (e.g., DispatchMechanisms).
    Union(AbiUnion),
    /// A constant value (e.g., ABI version, error codes).
    Const(AbiConst),
}

/// Information about a struct extracted from the ABI.
#[derive(Debug, Clone)]
pub struct AbiStruct {
    /// Struct name (e.g., "StringView").
    pub name: String,
    /// Struct fields.
    pub fields: Vec<AbiField>,
    /// Optional documentation comment.
    pub doc: Option<String>,
    /// Whether this struct has #[repr(C)].
    pub repr_c: bool,
    /// Expected size in bytes from the Rust layout.
    ///
    /// Populated from the known size table in generate.rs.
    /// Used to generate size assertions in SDK files.
    pub size_hint: Option<usize>,
}

/// Information about a struct field.
#[derive(Debug, Clone)]
pub struct AbiField {
    /// Field name.
    pub name: String,
    /// Rust type name (e.g., "*const u8", "usize").
    pub rust_type: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum extracted from the ABI.
#[derive(Debug, Clone)]
pub struct AbiEnum {
    /// Enum name (e.g., "DispatchType").
    pub name: String,
    /// Representation type (e.g., "u32", "u8").
    pub repr: String,
    /// Enum variants.
    pub variants: Vec<AbiVariant>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum variant.
#[derive(Debug, Clone)]
pub struct AbiVariant {
    /// Variant name.
    pub name: String,
    /// Optional discriminant value.
    pub value: Option<u64>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union extracted from the ABI.
#[derive(Debug, Clone)]
pub struct AbiUnion {
    /// Union name (e.g., "DispatchMechanisms").
    pub name: String,
    /// Union variants.
    pub variants: Vec<AbiUnionVariant>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union variant.
#[derive(Debug, Clone)]
pub struct AbiUnionVariant {
    /// Variant name.
    pub name: String,
    /// Variant type name.
    pub rust_type: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a constant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct AbiConst {
    /// Constant name (e.g., "POLYPLUG_ABI_VERSION").
    pub name: String,
    /// Rust type name (e.g., "u32", "u64").
    pub rust_type: String,
    /// Constant value as a string.
    pub value: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Complete collection of extracted ABI types.
#[derive(Debug, Clone, Default)]
pub struct AbiTypes {
    /// All structs.
    pub structs: Vec<AbiStruct>,
    /// All enums.
    pub enums: Vec<AbiEnum>,
    /// All unions.
    pub unions: Vec<AbiUnion>,
    /// All constants.
    pub consts: Vec<AbiConst>,
}

impl AbiTypes {
    /// Create an empty collection.
    pub fn new() -> AbiTypes {
        AbiTypes::default()
    }

    /// Add a struct.
    pub fn add_struct(&mut self, struct_info: AbiStruct) {
        self.structs.push(struct_info);
    }

    /// Add an enum.
    pub fn add_enum(&mut self, enum_info: AbiEnum) {
        self.enums.push(enum_info);
    }

    /// Add a union.
    pub fn add_union(&mut self, union_info: AbiUnion) {
        self.unions.push(union_info);
    }

    /// Add a constant.
    pub fn add_const(&mut self, const_info: AbiConst) {
        self.consts.push(const_info);
    }

    /// Merge another AbiTypes into this one.
    pub fn merge(&mut self, other: AbiTypes) {
        self.structs.extend(other.structs);
        self.enums.extend(other.enums);
        self.unions.extend(other.unions);
        self.consts.extend(other.consts);
    }

    /// Get all types as a flat vector.
    pub fn types(&self) -> Vec<AbiType> {
        let mut types: Vec<AbiType> = Vec::new();
        for s in &self.structs {
            types.push(AbiType::Struct(s.clone()));
        }
        for e in &self.enums {
            types.push(AbiType::Enum(e.clone()));
        }
        for u in &self.unions {
            types.push(AbiType::Union(u.clone()));
        }
        for c in &self.consts {
            types.push(AbiType::Const(c.clone()));
        }
        types
    }
}
