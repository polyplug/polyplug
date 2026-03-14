//! IR — intermediate representation types and validation for polyplugc.
//!
//! The IR is produced by the parser, validated (type resolution, contract IDs),
//! and then consumed by code generators.

use std::collections::HashMap;

use polyplug::abi::bundle_id as runtime_bundle_id;
use polyplug::abi::contract_id as runtime_contract_id;
use polyplug::abi::extension_id as runtime_extension_id;

use crate::error::CodegenError;

// ─── Version ─────────────────────────────────────────────────────────────

/// Semantic version with major.minor.patch components.
//
//  Major version is part of the contract identity (encoded in contract_id hash).
//  Minor version determines backward compatibility (new functions appended).
//  Patch version is informational only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Parse "major.minor.patch" or "major.minor" or "major" string.
    pub(crate) fn parse(s: &str) -> Result<Version, CodegenError> {
        let parts: Vec<&str> = s.split('.').collect();
        let parse_u32 = |p: &str| -> Result<u32, CodegenError> {
            p.parse::<u32>()
                .map_err(|_| CodegenError::ValidationFailed {
                    message: format!("invalid version component `{p}` in `{s}`"),
                })
        };
        let major: u32 = parse_u32(parts.first().copied().unwrap_or("0"))?;
        let minor: u32 = parse_u32(parts.get(1).copied().unwrap_or("0"))?;
        let patch: u32 = parse_u32(parts.get(2).copied().unwrap_or("0"))?;
        Ok(Version {
            major,
            minor,
            patch,
        })
    }

    /// Encode minor.patch as (minor << 16 | patch) for ABI storage.
    #[allow(dead_code)]
    pub(crate) fn minor_patch_encoded(&self) -> u32 {
        (self.minor << 16) | self.patch
    }
}

// ─── Primitive and ABI Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrimitiveType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Bool,
}

impl PrimitiveType {
    pub(crate) fn parse(s: &str) -> Option<PrimitiveType> {
        match s {
            "u8" => Some(PrimitiveType::U8),
            "u16" => Some(PrimitiveType::U16),
            "u32" => Some(PrimitiveType::U32),
            "u64" => Some(PrimitiveType::U64),
            "i8" => Some(PrimitiveType::I8),
            "i16" => Some(PrimitiveType::I16),
            "i32" => Some(PrimitiveType::I32),
            "i64" => Some(PrimitiveType::I64),
            "f32" => Some(PrimitiveType::F32),
            "f64" => Some(PrimitiveType::F64),
            "bool" => Some(PrimitiveType::Bool),
            _ => None,
        }
    }

    /// Rust type name.
    pub(crate) fn rust_name(&self) -> &'static str {
        match self {
            PrimitiveType::U8 => "u8",
            PrimitiveType::U16 => "u16",
            PrimitiveType::U32 => "u32",
            PrimitiveType::U64 => "u64",
            PrimitiveType::I8 => "i8",
            PrimitiveType::I16 => "i16",
            PrimitiveType::I32 => "i32",
            PrimitiveType::I64 => "i64",
            PrimitiveType::F32 => "f32",
            PrimitiveType::F64 => "f64",
            PrimitiveType::Bool => "bool",
        }
    }

    /// C/C++ type name.
    pub(crate) fn cpp_name(&self) -> &'static str {
        match self {
            PrimitiveType::U8 => "uint8_t",
            PrimitiveType::U16 => "uint16_t",
            PrimitiveType::U32 => "uint32_t",
            PrimitiveType::U64 => "uint64_t",
            PrimitiveType::I8 => "int8_t",
            PrimitiveType::I16 => "int16_t",
            PrimitiveType::I32 => "int32_t",
            PrimitiveType::I64 => "int64_t",
            PrimitiveType::F32 => "float",
            PrimitiveType::F64 => "double",
            PrimitiveType::Bool => "bool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AbiBuiltin {
    StringView,
    Buffer,
    Ptr,
    Void,
}

impl AbiBuiltin {
    pub(crate) fn parse(s: &str) -> Option<AbiBuiltin> {
        match s {
            "StringView" => Some(AbiBuiltin::StringView),
            "Buffer" => Some(AbiBuiltin::Buffer),
            "ptr" | "Ptr" => Some(AbiBuiltin::Ptr),
            "void" | "Void" => Some(AbiBuiltin::Void),
            _ => None,
        }
    }
}

// ─── Resolved Type References ─────────────────────────────────────────────────────

/// A resolved type reference.
#[derive(Debug, Clone)]
pub(crate) enum ResolvedTypeRef {
    Primitive(PrimitiveType),
    AbiType(AbiBuiltin),
    /// User-defined struct name (from api.toml).
    UserDefined(String),
}

// ─── Enum IR ────────────────────────────────────────────────────────────────

/// Repr type for an enum — the ABI-level integer type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReprType {
    U8,
    U16,
    U32,
    U64,
}

impl ReprType {
    /// Parse "u8" | "u16" | "u32" | "u64" string.
    pub(crate) fn parse(s: &str) -> Option<ReprType> {
        match s {
            "u8" => Some(ReprType::U8),
            "u16" => Some(ReprType::U16),
            "u32" => Some(ReprType::U32),
            "u64" => Some(ReprType::U64),
            _ => None,
        }
    }

    /// The Rust type name for this repr.
    pub(crate) fn rust_name(&self) -> &'static str {
        match self {
            ReprType::U8 => "u8",
            ReprType::U16 => "u16",
            ReprType::U32 => "u32",
            ReprType::U64 => "u64",
        }
    }

    /// The C/C++ type name for this repr.
    pub(crate) fn cpp_name(&self) -> &'static str {
        match self {
            ReprType::U8 => "uint8_t",
            ReprType::U16 => "uint16_t",
            ReprType::U32 => "uint32_t",
            ReprType::U64 => "uint64_t",
        }
    }

    /// The C# type name for this repr.
    pub(crate) fn cs_name(&self) -> &'static str {
        match self {
            ReprType::U8 => "byte",
            ReprType::U16 => "ushort",
            ReprType::U32 => "uint",
            ReprType::U64 => "ulong",
        }
    }
}

/// A validated enum variant.
#[derive(Debug, Clone)]
pub(crate) struct EnumVariant {
    pub name: String,
    /// Validated expression string, stored verbatim for codegen.
    pub value: String,
}

/// A fully validated enum definition.
#[derive(Debug)]
pub(crate) struct EnumDef {
    pub name: String,
    pub repr: ReprType,
    pub bitflag: bool,
    pub variants: Vec<EnumVariant>,
}

// ─── IR Structs ────────────────────────────────────────────────────────────────

/// A resolved user-defined flat struct.
#[derive(Debug)]
pub(crate) struct ResolvedType {
    pub name: String,
    pub fields: Vec<ResolvedField>,
}

#[derive(Debug)]
pub(crate) struct ResolvedField {
    pub name: String,
    pub ty: ResolvedTypeRef,
}

/// A resolved function parameter.
#[derive(Debug)]
pub(crate) struct ResolvedParam {
    pub name: String,
    pub ty: ResolvedTypeRef,
}

#[derive(Debug)]
pub(crate) struct ResolvedFunction {
    pub name: String,
    /// Sequential index matching declaration order.
    pub function_id: u32,
    pub params: Vec<ResolvedParam>,
    pub returns: Option<ResolvedTypeRef>,
}

#[derive(Debug)]
pub(crate) struct ResolvedContract {
    pub name: String,
    /// Precomputed FNV-1a hash of "name@major".
    pub contract_id: u64,
    #[allow(dead_code)]
    pub version: Version,
    pub functions: Vec<ResolvedFunction>,
}

#[derive(Debug)]
pub(crate) struct ResolvedPlugin {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub version: Version,
    #[allow(dead_code)]
    pub implements: Vec<String>,
    #[allow(dead_code)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PlatformKey {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedBundleFile {
    Single(String),
    PlatformMap(HashMap<PlatformKey, String>),
}

#[derive(Debug)]
pub(crate) struct ResolvedBundle {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub version: Version,
    #[allow(dead_code)]
    pub runtime: String,
    #[allow(dead_code)]
    pub file: ResolvedBundleFile,
    #[allow(dead_code)]
    pub plugins: Vec<ResolvedPlugin>,
    #[allow(dead_code)]
    pub bundle_id: u64,
    #[allow(dead_code)]
    pub dependencies: Vec<ResolvedDependency>,
    #[allow(dead_code)]
    pub needs_reinit_on_dep_reload: bool,
}

/// A resolved bundle dependency — either by contract (any provider) or by bundle (specific provider).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum ResolvedDependency {
    ByContract {
        contract: String,
        contract_id: u64,
        min_version: u32,
    },
    ByBundle {
        bundle: String,
        bundle_id: u64,
        contract: String,
        contract_id: u64,
        min_version: u32,
    },
}

/// The fully validated IR, ready for code generation.
#[derive(Debug)]
pub(crate) struct ValidatedIr {
    pub types: Vec<ResolvedType>,
    pub enums: Vec<EnumDef>,
    pub contracts: Vec<ResolvedContract>,
    #[allow(dead_code)]
    pub bundle: Option<ResolvedBundle>,
}

// ─── FNV-1a (re-exported from runtime) ───────────────────────────────────────────

/// Compute a contract ID: FNV-1a of "name@major".
pub(crate) fn compute_contract_id(name: &str, major: u32) -> u64 {
    runtime_contract_id(name, major)
}

/// Compute a bundle ID: FNV-1a hash of the bundle name.
pub(crate) fn compute_bundle_id(name: &str) -> u64 {
    runtime_bundle_id(name)
}

/// Compute an extension ID: FNV-1a lower 32 bits of name.
#[allow(dead_code)]
pub(crate) fn compute_extension_id(name: &str) -> u32 {
    runtime_extension_id(name)
}

// ─── Type Resolution ──────────────────────────────────────────────────────────────

/// Resolve a type string to a ResolvedTypeRef.
pub(crate) fn resolve_type_ref(
    type_str: &str,
    contract: &str,
    known_types: &[String],
) -> Result<ResolvedTypeRef, CodegenError> {
    if let Some(p) = PrimitiveType::parse(type_str) {
        return Ok(ResolvedTypeRef::Primitive(p));
    }
    if let Some(b) = AbiBuiltin::parse(type_str) {
        return Ok(ResolvedTypeRef::AbiType(b));
    }
    if known_types.contains(&type_str.to_owned()) {
        return Ok(ResolvedTypeRef::UserDefined(type_str.to_owned()));
    }
    Err(CodegenError::UnknownType {
        type_ref: type_str.to_owned(),
        contract: contract.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse() {
        let v: Version = Version::parse("1.2.3").expect("parse");
        assert_eq!(
            v,
            Version {
                major: 1,
                minor: 2,
                patch: 3
            }
        );
    }

    #[test]
    fn version_minor_patch_encoded() {
        let v: Version = Version {
            major: 1,
            minor: 3,
            patch: 5,
        };
        assert_eq!(v.minor_patch_encoded(), (3 << 16) | 5);
    }

    #[test]
    fn primitive_type_roundtrip() {
        let t: PrimitiveType = PrimitiveType::parse("u32").expect("parse u32");
        assert_eq!(t.rust_name(), "u32");
        assert_eq!(t.cpp_name(), "uint32_t");
    }

    #[test]
    fn resolve_type_ref_primitive() {
        let t: ResolvedTypeRef = resolve_type_ref("u64", "my.contract", &[]).expect("resolve u64");
        assert!(matches!(t, ResolvedTypeRef::Primitive(PrimitiveType::U64)));
    }

    #[test]
    fn resolve_type_ref_unknown() {
        let result: Result<ResolvedTypeRef, _> =
            resolve_type_ref("MyUnknownType", "my.contract", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn contract_id_uses_fnv1a() {
        let id1: u64 = compute_contract_id("image.decode", 1);
        let id2: u64 = compute_contract_id("image.decode", 1);
        assert_eq!(id1, id2);
        assert_ne!(
            compute_contract_id("image.decode", 1),
            compute_contract_id("image.decode", 2)
        );
    }

    #[test]
    fn bundle_id_uses_fnv1a() {
        // FNV-1a of "test-bundle" must be deterministic and non-zero
        let id: u64 = compute_bundle_id("test-bundle");
        assert_ne!(id, 0);
        // Same input → same output
        assert_eq!(id, compute_bundle_id("test-bundle"));
        // Different inputs → different outputs (basic collision check)
        assert_ne!(compute_bundle_id("bundle-a"), compute_bundle_id("bundle-b"));
    }

    #[test]
    fn repr_type_parse_roundtrip() {
        let r: ReprType = ReprType::parse("u32").expect("parse u32");
        assert_eq!(r.rust_name(), "u32");
        assert_eq!(r.cpp_name(), "uint32_t");
        assert_eq!(r.cs_name(), "uint");
    }

    #[test]
    fn repr_type_parse_u64() {
        let r: Option<ReprType> = ReprType::parse("u64");
        assert!(matches!(r, Some(ReprType::U64)));
    }

    #[test]
    fn repr_type_parse_invalid() {
        let r: Option<ReprType> = ReprType::parse("i32");
        assert!(r.is_none());
    }
}
