/// Information about a constant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct ConstantInfo {
    /// Constant name (e.g., "POLYPLUG_ABI_VERSION").
    pub name: String,
    /// Constant value as a string (for language-specific formatting).
    pub value: String,
    /// Constant type (e.g., "u32").
    pub type_name: String,
}

/// Information about a struct field extracted from the ABI.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Field name.
    pub name: String,
    /// Field type name.
    pub type_name: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a struct extracted from the ABI.
#[derive(Debug, Clone)]
pub struct StructInfo {
    /// Struct name (e.g., "StringView").
    pub name: String,
    /// Struct fields.
    pub fields: Vec<FieldInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum variant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct VariantInfo {
    /// Variant name.
    pub name: String,
    /// Optional discriminant value.
    pub value: Option<i64>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about an enum extracted from the ABI.
#[derive(Debug, Clone)]
pub struct EnumInfo {
    /// Enum name (e.g., "DispatchType").
    pub name: String,
    /// Enum variants.
    pub variants: Vec<VariantInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union variant extracted from the ABI.
#[derive(Debug, Clone)]
pub struct UnionVariantInfo {
    /// Variant name.
    pub name: String,
    /// Variant type name.
    pub type_name: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a union extracted from the ABI.
#[derive(Debug, Clone)]
pub struct UnionInfo {
    /// Union name (e.g., "PluginDispatch").
    pub name: String,
    /// Union variants.
    pub variants: Vec<UnionVariantInfo>,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Information about a function extracted from the ABI.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name.
    pub name: String,
    /// Return type name.
    pub return_type: String,
    /// Optional documentation comment.
    pub doc: Option<String>,
}

/// Complete ABI type information extracted from `polyplug_abi`.
///
/// This struct holds all the type information needed to generate bindings
/// in any target language. Language-specific generators use this data to
/// produce idiomatic code.
#[derive(Debug, Clone, Default)]
pub struct AbiInfo {
    /// ABI constants (version, error codes).
    pub constants: Vec<ConstantInfo>,
    /// ABI structs (StringView, Buffer, etc.).
    pub structs: Vec<StructInfo>,
    /// ABI enums (DispatchType).
    pub enums: Vec<EnumInfo>,
    /// ABI unions (PluginDispatch).
    pub unions: Vec<UnionInfo>,
    /// ABI functions (FNV-1a hash helpers).
    pub functions: Vec<FunctionInfo>,
}

impl AbiInfo {
    /// Create an empty `AbiInfo`.
    pub fn new() -> AbiInfo {
        AbiInfo::default()
    }

    /// Add a constant.
    pub fn add_constant(&mut self, constant: ConstantInfo) {
        self.constants.push(constant);
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
    pub fn add_function(&mut self, function: FunctionInfo) {
        self.functions.push(function);
    }
}
