//! Language-agnostic data types for ABI code generation.
//!
//! This module defines the `Item` enum and associated info structs that represent
//! ABI types extracted from Rust source. These types are used by the `CodeGenerator`
//! trait to produce language-specific bindings.

use std::collections::HashMap;

/// A single ABI item to be generated.
#[derive(Debug, Clone)]
pub enum Item {
    /// A constant value (e.g., ABI version, error codes).
    Const(ConstInfo),
    /// A struct definition (e.g., StringView, Buffer).
    Struct(StructInfo),
    /// An enum definition (e.g., DispatchType).
    Enum(EnumInfo),
    /// A union definition (e.g., PluginDispatch).
    Union(UnionInfo),
    /// A function definition (e.g., FNV-1a hash helpers).
    Function(FunctionInfo),
}

/// Information about a constant.
#[derive(Debug, Clone)]
pub struct ConstInfo {
    /// Constant name (e.g., "POLYPLUG_ABI_VERSION").
    pub name: String,
    /// Rust type name (e.g., "u32", "u64").
    pub rust_type: String,
    /// Constant value as a string (for language-specific formatting).
    pub value: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a struct field.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Field name.
    pub name: String,
    /// Rust type name (e.g., "*const u8", "usize").
    pub rust_type: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a struct.
#[derive(Debug, Clone)]
pub struct StructInfo {
    /// Struct name (e.g., "StringView").
    pub name: String,
    /// Struct fields.
    pub fields: Vec<FieldInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
    /// Additional attributes (e.g., repr, derive).
    pub attributes: Vec<String>,
    /// Expected size in bytes from the Rust layout, if known.
    ///
    /// Used to generate size assertions in SDK files.
    /// Populated from the known size table in the build script.
    pub size_hint: Option<usize>,
}

/// Information about an enum variant.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    /// Variant name.
    pub name: String,
    /// Optional discriminant value.
    pub value: Option<u64>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum.
#[derive(Debug, Clone)]
pub struct EnumInfo {
    /// Enum name (e.g., "DispatchType").
    pub name: String,
    /// Representation type (e.g., "u32", "u8").
    pub repr: String,
    /// Enum variants.
    pub variants: Vec<EnumVariant>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union variant.
#[derive(Debug, Clone)]
pub struct UnionVariant {
    /// Variant name.
    pub name: String,
    /// Variant type name.
    pub type_name: String,
}

/// Information about a union.
#[derive(Debug, Clone)]
pub struct UnionInfo {
    /// Union name (e.g., "PluginDispatch").
    pub name: String,
    /// Union variants.
    pub variants: Vec<UnionVariant>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a function parameter.
#[derive(Debug, Clone)]
pub struct ParamInfo {
    /// Parameter name.
    pub name: String,
    /// Rust type name.
    pub rust_type: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a function.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name.
    pub name: String,
    /// Function parameters.
    pub params: Vec<ParamInfo>,
    /// Return type (None for void).
    pub return_type: Option<String>,
    /// Whether this is a constexpr function (C++ specific).
    pub is_constexpr: bool,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Complete collection of ABI items.
#[derive(Debug, Clone, Default)]
pub struct AbiItems {
    /// All constants.
    pub consts: Vec<ConstInfo>,
    /// All structs.
    pub structs: Vec<StructInfo>,
    /// All enums.
    pub enums: Vec<EnumInfo>,
    /// All unions.
    pub unions: Vec<UnionInfo>,
    /// All functions.
    pub functions: Vec<FunctionInfo>,
}

impl AbiItems {
    /// Create an empty collection.
    pub fn new() -> AbiItems {
        AbiItems::default()
    }

    /// Add a constant.
    pub fn add_const(&mut self, const_info: ConstInfo) {
        self.consts.push(const_info);
    }

    /// Add a struct.
    pub fn add_struct(&mut self, struct_info: StructInfo) {
        self.structs.push(struct_info);
    }

    /// Add an enum.
    pub fn add_enum(&mut self, enum_info: EnumInfo) {
        self.enums.push(enum_info);
    }

    /// Add a union.
    pub fn add_union(&mut self, union_info: UnionInfo) {
        self.unions.push(union_info);
    }

    /// Add a function.
    pub fn add_function(&mut self, function_info: FunctionInfo) {
        self.functions.push(function_info);
    }

    /// Get all items as a flat vector.
    pub fn items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = Vec::new();
        for c in &self.consts {
            items.push(Item::Const(c.clone()));
        }
        for s in &self.structs {
            items.push(Item::Struct(s.clone()));
        }
        for e in &self.enums {
            items.push(Item::Enum(e.clone()));
        }
        for u in &self.unions {
            items.push(Item::Union(u.clone()));
        }
        for f in &self.functions {
            items.push(Item::Function(f.clone()));
        }
        items
    }

    /// Create a struct lookup map by name.
    pub fn struct_map(&self) -> HashMap<&str, &StructInfo> {
        let mut map: HashMap<&str, &StructInfo> = HashMap::new();
        for s in &self.structs {
            map.insert(&s.name, s);
        }
        map
    }

    /// Create an enum lookup map by name.
    pub fn enum_map(&self) -> HashMap<&str, &EnumInfo> {
        let mut map: HashMap<&str, &EnumInfo> = HashMap::new();
        for e in &self.enums {
            map.insert(&e.name, e);
        }
        map
    }

    /// Create a union lookup map by name.
    pub fn union_map(&self) -> HashMap<&str, &UnionInfo> {
        let mut map: HashMap<&str, &UnionInfo> = HashMap::new();
        for u in &self.unions {
            map.insert(&u.name, u);
        }
        map
    }
}
