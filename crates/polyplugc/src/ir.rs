//! IR — intermediate representation types and validation for polyplugc.
//!
//! The IR is produced by the parser, validated (type resolution, contract IDs),
//! and then consumed by code generators.

use polyplug_codegen::PolyplugcError;
use polyplug_codegen::ResolvedBundleFile;

// ─── Version ─────────────────────────────────────────────────────────────

/// Semantic version with major.minor.patch components.
//
//  Major version is part of the contract identity (encoded in contract_id hash).
//  Minor version determines backward compatibility (new functions appended).
//  Patch version is informational only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Parse "major.minor.patch" or "major.minor" or "major" string.
    pub fn parse(s: &str) -> Result<Version, PolyplugcError> {
        let parts: Vec<&str> = s.split('.').collect();
        let parse_u32 = |p: &str| -> Result<u32, PolyplugcError> {
            p.parse::<u32>()
                .map_err(|_| PolyplugcError::ValidationFailed {
                    message: format!("invalid version component `{p}` in `{s}`"),
                })
        };
        let major: u32 = parse_u32(parts.first().copied().unwrap_or("0"))?;
        let minor: u32 = parse_u32(parts.get(1).copied().unwrap_or("0"))?;
        let patch: u32 = parse_u32(parts.get(2).copied().unwrap_or("0"))?;

        // Validate minor and patch fit in u16 for ABI encoding (minor << 16 | patch)
        const U16_MAX: u32 = u16::MAX as u32; // 65535
        if minor > U16_MAX {
            return Err(PolyplugcError::VersionOverflow {
                component: "minor".to_owned(),
                value: minor,
                version_str: s.to_owned(),
                location: None,
                suggestion: None,
            });
        }
        if patch > U16_MAX {
            return Err(PolyplugcError::VersionOverflow {
                component: "patch".to_owned(),
                value: patch,
                version_str: s.to_owned(),
                location: None,
                suggestion: None,
            });
        }

        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

// ─── Primitive and ABI Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveType {
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
    pub fn rust_name(&self) -> &'static str {
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
    pub fn cpp_name(&self) -> &'static str {
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
pub enum AbiBuiltin {
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
pub enum ResolvedTypeRef {
    Primitive(PrimitiveType),
    AbiType(AbiBuiltin),
    /// User-defined struct name (from api.toml).
    UserDefined(String),
}

// ─── Enum IR ────────────────────────────────────────────────────────────────

/// Repr type for an enum — the ABI-level integer type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReprType {
    U8,
    U16,
    U32,
    U64,
}

impl ReprType {
    /// Parse "u8" | "u16" | "u32" | "u64" string.
    pub fn parse(s: &str) -> Option<ReprType> {
        match s {
            "u8" => Some(ReprType::U8),
            "u16" => Some(ReprType::U16),
            "u32" => Some(ReprType::U32),
            "u64" => Some(ReprType::U64),
            _ => None,
        }
    }

    /// The Rust type name for this repr.
    pub fn rust_name(&self) -> &'static str {
        match self {
            ReprType::U8 => "u8",
            ReprType::U16 => "u16",
            ReprType::U32 => "u32",
            ReprType::U64 => "u64",
        }
    }

    /// The C/C++ type name for this repr.
    pub fn cpp_name(&self) -> &'static str {
        match self {
            ReprType::U8 => "uint8_t",
            ReprType::U16 => "uint16_t",
            ReprType::U32 => "uint32_t",
            ReprType::U64 => "uint64_t",
        }
    }

    /// The C# type name for this repr.
    pub fn cs_name(&self) -> &'static str {
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
pub struct EnumVariant {
    pub name: String,
    /// Validated expression string, stored verbatim for codegen.
    pub value: String,
}

/// A fully validated enum definition.
#[derive(Debug)]
pub struct EnumDef {
    pub name: String,
    pub repr: ReprType,
    pub bitflag: bool,
    pub variants: Vec<EnumVariant>,
}

// ─── IR Structs ────────────────────────────────────────────────────────────────

/// A resolved user-defined flat struct.
#[derive(Debug)]
pub struct ResolvedType {
    pub name: String,
    pub fields: Vec<ResolvedField>,
}

#[derive(Debug)]
pub struct ResolvedField {
    pub name: String,
    pub ty: ResolvedTypeRef,
}

/// A resolved function parameter.
#[derive(Debug)]
pub struct ResolvedParam {
    pub name: String,
    pub ty: ResolvedTypeRef,
}

#[derive(Debug)]
pub struct ResolvedFunction {
    pub name: String,
    /// Sequential index matching declaration order.
    pub function_id: u32,
    pub params: Vec<ResolvedParam>,
    pub returns: Option<ResolvedTypeRef>,
}

#[derive(Debug)]
pub struct ResolvedContract {
    pub name: String,
    /// Precomputed FNV-1a hash of "guest_contract:name@major".
    pub contract_id: u64,
    pub version: Version,
    pub functions: Vec<ResolvedFunction>,
}

/// A resolved host contract definition.
/// Host contracts define APIs that the host provides to plugins.
#[derive(Debug)]
pub struct ResolvedHostContract {
    pub name: String,
    /// Precomputed FNV-1a hash of "host_contract:name@major".
    pub contract_id: u64,
    pub version: Version,
    /// Whether this contract provides a singleton instance.
    /// If true, `get_host_contract` returns the same instance for all callers.
    pub singleton: bool,
    pub functions: Vec<ResolvedFunction>,
}

#[derive(Debug)]
pub struct ResolvedPlugin {
    pub name: String,
    pub implements: Vec<String>,
    pub optional: Vec<String>,
}

#[derive(Debug)]
pub struct ResolvedBundle {
    pub name: String,
    pub version: Version,
    pub loader: String,
    pub file: ResolvedBundleFile,
    pub plugins: Vec<ResolvedPlugin>,
    pub bundle_id: u64,
    pub dependencies: Vec<ResolvedDependency>,
    pub needs_reinit_on_dep_reload: bool,
}

/// A resolved bundle dependency — either by contract (any provider) or by bundle (specific provider).
#[derive(Debug, Clone)]
pub enum ResolvedDependency {
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
pub struct ValidatedIr {
    pub types: Vec<ResolvedType>,
    pub enums: Vec<EnumDef>,
    pub contracts: Vec<ResolvedContract>,
    pub host_contracts: Vec<ResolvedHostContract>,
    pub bundle: Option<ResolvedBundle>,
}

// ─── Type Resolution ──────────────────────────────────────────────────────────────

// ─── Type Resolution ──────────────────────────────────────────────────────────────

/// Resolve a type string to a ResolvedTypeRef.
pub fn resolve_type_ref(
    type_str: &str,
    contract: &str,
    known_types: &[String],
) -> Result<ResolvedTypeRef, PolyplugcError> {
    if let Some(p) = PrimitiveType::parse(type_str) {
        return Ok(ResolvedTypeRef::Primitive(p));
    }
    if let Some(b) = AbiBuiltin::parse(type_str) {
        return Ok(ResolvedTypeRef::AbiType(b));
    }
    if known_types.contains(&type_str.to_owned()) {
        return Ok(ResolvedTypeRef::UserDefined(type_str.to_owned()));
    }
    Err(PolyplugcError::UnknownType {
        type_ref: type_str.to_owned(),
        contract: contract.to_owned(),
        location: None,
        suggestion: None,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use polyplug_utils::{bundle_id, guest_contract_id};

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
    fn version_parse_minor_overflow_returns_error() {
        let result: Result<Version, PolyplugcError> = Version::parse("1.65536.0");
        assert!(result.is_err());
        let err: PolyplugcError = result.expect_err("should error on minor overflow");
        assert!(
            matches!(err, PolyplugcError::VersionOverflow { component, .. } if component == "minor")
        );
    }

    #[test]
    fn version_parse_patch_overflow_returns_error() {
        let result: Result<Version, PolyplugcError> = Version::parse("1.0.65536");
        assert!(result.is_err());
        let err: PolyplugcError = result.expect_err("should error on patch overflow");
        assert!(
            matches!(err, PolyplugcError::VersionOverflow { component, .. } if component == "patch")
        );
    }

    #[test]
    fn version_parse_max_valid_minor_patch() {
        let v: Version = Version::parse("1.65535.65535").expect("parse max valid");
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 65535);
        assert_eq!(v.patch, 65535);
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
        let id1: u64 = guest_contract_id("image.decode", 1);
        let id2: u64 = guest_contract_id("image.decode", 1);
        assert_eq!(id1, id2);
        assert_ne!(
            guest_contract_id("image.decode", 1),
            guest_contract_id("image.decode", 2)
        );
    }

    #[test]
    fn contract_id_golden_values() {
        // Golden values for "guest_contract:name@version" FNV-1a hash
        // These match polyplug_utils::GuestContractId::new()
        assert_eq!(
            guest_contract_id("image.decode", 1),
            18154885241241252316_u64
        );
        assert_eq!(
            guest_contract_id("audio.encode", 2),
            6632138859905976100_u64
        );
    }

    #[test]
    fn codegen_contract_id_deterministic() {
        // guest_contract_id must produce consistent results
        let id1: u64 = guest_contract_id("image.decode", 1);
        let id2: u64 = guest_contract_id("image.decode", 1);
        assert_eq!(id1, id2);
    }

    #[test]
    fn bundle_id_uses_fnv1a() {
        // FNV-1a of "test-bundle" must be deterministic and non-zero
        let id: u64 = bundle_id("test-bundle");
        assert_ne!(id, 0);
        // Same input → same output
        assert_eq!(id, bundle_id("test-bundle"));
        // Different inputs → different outputs (basic collision check)
        assert_ne!(bundle_id("bundle-a"), bundle_id("bundle-b"));
    }

    #[test]
    fn bundle_id_golden_values() {
        // Golden: FNV-1a of "my-bundle"
        assert_eq!(bundle_id("my-bundle"), 0xfe6226876e3a35b2_u64);
        // Golden: FNV-1a of "polyplug-core"
        assert_eq!(bundle_id("polyplug-core"), 0x6ef4aee714f5f991_u64);
    }

    #[test]
    fn codegen_bundle_id_deterministic() {
        // bundle_id must produce consistent results
        let id1: u64 = bundle_id("my-bundle");
        let id2: u64 = bundle_id("my-bundle");
        assert_eq!(id1, id2);
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
