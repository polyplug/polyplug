#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, InternalRustGenerateConfig, Lang, OutputDestination, OutputLayout, Side,
    ValidatedImport, generate, generate_internal_rust, write_output,
};
use polyplug_utils::bundle_id;
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_path(crate_dir: &str) -> String {
    workspace_root()
        .join(crate_dir)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn generated_consumer_source(source: String) -> String {
    source
        .replace(
            "INTERNAL_GENERATION_FINGERPRINT",
            "_POLYPLUG_INTERNAL_GENERATION_FINGERPRINT",
        )
        .lines()
        .filter(|line| !line.contains("assert_eq!(DOMAIN_FINGERPRINT"))
        .map(|line| format!("{line}\n"))
        .collect()
}

#[test]
fn generated_internal_rust_same_contract_providers_dispatch_statefully_and_unload() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Cold\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Hot\"\nvalue = \"2\"\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"mode\", type = \"Mode\" }, { name = \"modes\", type = \"Array<Mode>\" }]\n\n[[guest_contract]]\nname = \"platform.plugin\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"cycle\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"generated_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"first\"\nimplements = [\"platform.plugin@1.0\"]\n\n[[plugin]]\nname = \"second\"\nimplements = [\"platform.plugin@1.0\"]\n",
    )
    .expect("write internal bundle TOML");

    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout::unified(),
    })
    .expect("generate Rust internal-plugin bindings");
    let fingerprints: Vec<&str> = generated
        .files
        .iter()
        .filter_map(|file| {
            file.content.lines().find(|line| {
                line.starts_with("pub const _POLYPLUG_INTERNAL_GENERATION_FINGERPRINT:")
            })
        })
        .collect();
    assert!(
        fingerprints.len() >= 5 && fingerprints.windows(2).all(|pair| pair[0] == pair[1]),
        "generated internal partitions must expose one fingerprint: {fingerprints:?}"
    );
    let interfaces = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/interfaces.rs"))
        .expect("generated internal interfaces")
        .content
        .as_str();
    assert!(
        interfaces.contains("super::types::Mode::Cold => super::domain::Mode::Cold")
            && interfaces.contains("super::domain::Mode::Hot => super::types::Mode::Hot"),
        "nominal enum values must cross the ABI boundary through explicit mappings: {interfaces}"
    );
    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write generated bindings");
    let generated_root = Path::new("internal").join(format!(
        "generated_internal_plugin-{:016x}",
        bundle_id("generated_internal_plugin")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    assert!(
        generated
            .files
            .iter()
            .any(|file| file.path == generated_module_path),
        "generated internal root module"
    );
    fs::rename(
        source_dir.join(&generated_root),
        source_dir.join("generated"),
    )
    .expect("place stable generated module root");

    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated_internal_plugin_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer Cargo.toml");
    let consumer_source = String::from("mod generated;\n")
        + r#"use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use polyplug::Runtime;
use polyplug_guest::GuestError;

struct Provider {
    next: AtomicU32,
}

impl Provider {
    fn new(next: u32) -> Self {
        Self { next: AtomicU32::new(next) }
    }
}

impl generated::guest::guest_contracts::PlatformPluginContract for Provider {
    fn cycle(
        &self,
        value: generated::guest::domain::Envelope,
    ) -> Result<generated::guest::domain::Envelope, GuestError> {
        let mode = if self.next.fetch_add(1, Ordering::Relaxed) % 2 == 0 {
            generated::guest::domain::Mode::Hot
        } else {
            generated::guest::domain::Mode::Cold
        };
        Ok(generated::guest::domain::Envelope {
            mode,
            modes: value.modes,
        })
    }
}

fn main() {
    let runtime = Arc::new(Runtime::builder().build().expect("build runtime"));
    let mut registration = generated::guest::init::register(
        Arc::clone(&runtime),
        generated::guest::interfaces::InternalProviders {
            first_platform_plugin: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn generated::guest::guest_contracts::PlatformPluginContract> { Box::new(Provider::new(10)) }),
            second_platform_plugin: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn generated::guest::guest_contracts::PlatformPluginContract> { Box::new(Provider::new(20)) }),
        },
    )
    .expect("register internal plugin");
    let input = generated::host::types::Envelope {
        mode: generated::host::types::Mode::Cold,
        modes: generated::host::types::ArrayOf_Mode {
            items: 0,
            len: 0,
        },
    };
    assert_eq!(
        registration
            .first_platform_plugin
            .cycle(&input)
            .expect("call first provider")
            .mode,
        generated::host::types::Mode::Hot
    );
    assert_eq!(
        registration
            .first_platform_plugin
            .cycle(&input)
            .expect("preserve first provider state")
            .mode,
        generated::host::types::Mode::Cold
    );
    assert_eq!(
        registration
            .second_platform_plugin
            .cycle(&input)
            .expect("call second provider")
            .mode,
        generated::host::types::Mode::Hot
    );
    let bundle_id = registration.bundle_id;
    drop(registration);
    runtime.unload_bundle(bundle_id).expect("unload after callers tear down");
}
"#;
    fs::write(source_dir.join("main.rs"), consumer_source).expect("write consumer source");

    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&crate_root)
        .output()
        .expect("run generated consumer check");
    assert!(
        output.status.success(),
        "generated internal Rust bindings did not register, dispatch, and unload:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_internal_rust_tagged_enum_projection_dispatches_all_variants() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[enum]]\nname = \"Kind\"\nrepr = \"u32\"\nlangs = { rust = { serde = \"human-name-binary-discriminant\" } }\n\n[[enum.variants]]\nname = \"Empty\"\nvalue = \"17\"\nlangs = { rust = { primary_name = \"none\", default = true } }\n\n[[enum.variants]]\nname = \"Boolean\"\nvalue = \"305419896\"\n\n[[enum.variants]]\nname = \"Integer\"\nvalue = \"2596069104\"\n\n[[enum.variants]]\nname = \"Decimal\"\nvalue = \"3735928559\"\n\n[[enum.variants]]\nname = \"Text\"\nvalue = \"4294967294\"\n\n[[enum]]\nname = \"SmallKind\"\nrepr = \"u8\"\nlangs = { rust = { serde = \"human-name-binary-discriminant\" } }\n\n[[enum.variants]]\nname = \"Tiny\"\nvalue = \"7\"\nlangs = { rust = { default = true } }\n\n[[enum.variants]]\nname = \"Large\"\nvalue = \"251\"\n\n[[types]]\nname = \"Value\"\nlangs = { rust = { tagged_enum = { tag_field = \"kind\", variants = [{ tag = \"Empty\", name = \"None\" }, { tag = \"Boolean\", name = \"Bool\", payload = \"bool_value\" }, { tag = \"Integer\", name = \"Int\", payload = \"int_value\" }, { tag = \"Decimal\", name = \"Float\", payload = \"float_value\" }, { tag = \"Text\", name = \"String\", payload = \"string_value\" }] } } }\nfields = [{ name = \"kind\", type = \"Kind\" }, { name = \"bool_value\", type = \"bool\" }, { name = \"int_value\", type = \"i64\" }, { name = \"float_value\", type = \"f64\" }, { name = \"string_value\", type = \"StringView\" }]\n\n[[guest_contract]]\nname = \"projection.value\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"roundtrip\"\nparams = [{ name = \"value\", type = \"Value\" }]\nreturn = \"Value\"\n",
    )
    .expect("write tagged projection API");
    let api_text = fs::read_to_string(&api)
        .expect("read tagged projection API")
        .replace(
            "{ tag = \"Empty\", name = \"None\" }",
            "{ tag = \"Empty\", name = \"None\", default = true }",
        )
        .replace(
            "primary_name = \"none\", default = true",
            "primary_name = \"none\", aliases = [\"empty\"], default = true",
        )
        .replace(
            "{ name = \"string_value\", type = \"StringView\" }]",
            "{ name = \"string_value\", type = \"StringView\" }, { name = \"values\", type = \"Array<bool>\", langs = { rust = { empty_sequence_as_null = true } } }]",
        )
        .replace(
            "\n[[types]]\nname = \"Value\"",
            "\n[[enum]]\nname = \"MediumKind\"\nrepr = \"u16\"\nlangs = { rust = { serde = \"human-name-binary-discriminant\" } }\n\n[[enum.variants]]\nname = \"Medium\"\nvalue = \"513\"\n\n[[enum.variants]]\nname = \"Maximum\"\nvalue = \"65535\"\n\n[[enum]]\nname = \"LargeKind\"\nrepr = \"u64\"\nlangs = { rust = { serde = \"human-name-binary-discriminant\" } }\n\n[[enum.variants]]\nname = \"Large\"\nvalue = \"4294967297\"\n\n[[enum.variants]]\nname = \"Maximum\"\nvalue = \"18446744073709551615\"\n\n[[types]]\nname = \"Value\"",
        );
    fs::write(&api, api_text).expect("add tagged projection default");
    fs::write(
        &bundle,
        "[bundle]\nname = \"tagged_projection_dispatch\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[dependency]]\nkind = \"contract\"\ncontract = \"projection.value\"\nmin_version = \"1.0.0\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"projection.value@1.0\"]\n",
    )
    .expect("write tagged projection bundle");
    let declarations_root = temp.path().join("declarations");
    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: declarations_root.clone(),
                import: ValidatedImport::parse(Lang::Rust, "crate::domain").expect("domain import"),
            },
            guest_contracts: OutputDestination::Emit {
                root: declarations_root.clone(),
                import: ValidatedImport::parse(Lang::Rust, "crate::guest_contracts")
                    .expect("guest-contract import"),
            },
        },
    })
    .expect("generate split tagged projection bindings");
    let domain = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/domain.rs"))
        .expect("generated domain declarations")
        .content
        .as_str();
    assert!(
        domain.contains("pub enum Value")
            && domain.contains("None,")
            && domain.contains("Bool(bool)")
            && domain.contains("Int(i64)")
            && domain.contains("Float(f64)")
            && domain.contains("String(String)"),
        "tagged projection must expose authored domain variants: {domain}"
    );
    assert!(
        domain.contains("serializer.serialize_u32(")
            && domain.contains("serializer.serialize_u8(")
            && !domain.contains("derive(Debug, Clone, PartialEq, Eq")
            && !domain.contains("derive(Debug, Clone, PartialEq, Hash"),
        "domain must use authored enum repr widths without invalid float derives: {domain}"
    );
    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write generated bindings");
    let generated_root = Path::new("internal").join(format!(
        "tagged_projection_dispatch-{:016x}",
        bundle_id("tagged_projection_dispatch")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    let domain_module_path = declarations_root
        .join(&generated_root)
        .join("guest/domain.rs");
    let guest_contracts_module_path = declarations_root
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"tagged_projection_dispatch_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\npostcard = {{ version = \"1\", features = [\"use-std\"] }}\nserde = \"1\"\nserde_json = \"1\"\nserde_yaml = \"0.9\"\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer Cargo.toml");
    let consumer_source = format!(
        "#[path = {domain_module_path:?}]\npub mod domain;\n#[path = {guest_contracts_module_path:?}]\npub mod guest_contracts;\n#[path = {generated_module_path:?}]\nmod generated;\n"
    ) + r#"use std::fmt::{Display, Formatter};
use std::sync::Arc;

use polyplug::Runtime;
use polyplug_guest::{GuestError, HostContext};
use serde::de::{Deserializer, Visitor};
use serde::ser::{Impossible, Serializer};
use serde::{Deserialize, Serialize};
use generated::guest::domain as generated_domain;
use generated::guest::guest_contracts as generated_guest_contracts;
use generated::host::domain as host_domain;

#[derive(Debug)]
struct BinaryError;

impl Display for BinaryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("unsupported binary operation")
    }
}

impl std::error::Error for BinaryError {}

impl serde::ser::Error for BinaryError {
    fn custom<T: Display>(_message: T) -> Self {
        Self
    }
}

impl serde::de::Error for BinaryError {
    fn custom<T: Display>(_message: T) -> Self {
        Self
    }
}

#[derive(Default)]
struct BinarySink(Vec<u8>);

impl Serializer for &mut BinarySink {
    type Ok = ();
    type Error = BinaryError;
    type SerializeSeq = Impossible<(), BinaryError>;
    type SerializeTuple = Impossible<(), BinaryError>;
    type SerializeTupleStruct = Impossible<(), BinaryError>;
    type SerializeTupleVariant = Impossible<(), BinaryError>;
    type SerializeMap = Impossible<(), BinaryError>;
    type SerializeStruct = Impossible<(), BinaryError>;
    type SerializeStructVariant = Impossible<(), BinaryError>;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        self.0.push(value);
        Ok(())
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        self.0.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        self.0.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        self.0.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_bool(self, _value: bool) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_i8(self, _value: i8) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_i16(self, _value: i16) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_i32(self, _value: i32) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_i64(self, _value: i64) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_i128(self, _value: i128) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_u128(self, _value: u128) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_f32(self, _value: f32) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_f64(self, _value: f64) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_char(self, _value: char) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_str(self, _value: &str) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_unit_variant(self, _name: &'static str, _index: u32, _variant: &'static str) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _name: &'static str, _value: &T) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _name: &'static str, _index: u32, _variant: &'static str, _value: &T) -> Result<Self::Ok, Self::Error> { Err(BinaryError) }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> { Err(BinaryError) }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> { Err(BinaryError) }
    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> { Err(BinaryError) }
    fn serialize_tuple_variant(self, _name: &'static str, _index: u32, _variant: &'static str, _len: usize) -> Result<Self::SerializeTupleVariant, Self::Error> { Err(BinaryError) }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> { Err(BinaryError) }
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct, Self::Error> { Err(BinaryError) }
    fn serialize_struct_variant(self, _name: &'static str, _index: u32, _variant: &'static str, _len: usize) -> Result<Self::SerializeStructVariant, Self::Error> { Err(BinaryError) }
}

enum BinaryNumber {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

impl<'de> Deserializer<'de> for BinaryNumber {
    type Error = BinaryError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(BinaryError)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::U8(value) => visitor.visit_u8(value),
            _ => Err(BinaryError),
        }
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::U16(value) => visitor.visit_u16(value),
            _ => Err(BinaryError),
        }
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::U32(value) => visitor.visit_u32(value),
            _ => Err(BinaryError),
        }
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::U64(value) => visitor.visit_u64(value),
            _ => Err(BinaryError),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

struct Provider;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Settings {
    #[serde(default, deserialize_with = "domain::empty_sequence_as_null", serialize_with = "domain::serialize_empty_sequence_as_null")]
    values: Vec<bool>,
}

impl generated_guest_contracts::ProjectionValueContract for Provider {
    fn roundtrip(&self, value: domain::Value) -> Result<domain::Value, GuestError> {
        match &value {
            domain::Value::None => {}
            domain::Value::Bool(value) => assert!(*value),
            domain::Value::Int(value) => assert_eq!(*value, -47),
            domain::Value::Float(value) => assert_eq!(*value, 13.25),
            domain::Value::String(value) => assert_eq!(value, "arena-string"),
        }
        Ok(value)
    }
}


fn main() {
    for (kind, expected) in [
        (domain::Kind::Empty, [17, 0, 0, 0]),
        (domain::Kind::Boolean, [0x78, 0x56, 0x34, 0x12]),
        (domain::Kind::Integer, [0xF0, 0xDE, 0xBC, 0x9A]),
        (domain::Kind::Decimal, [0xEF, 0xBE, 0xAD, 0xDE]),
        (domain::Kind::Text, [0xFE, 0xFF, 0xFF, 0xFF]),
    ] {
        let mut bytes = BinarySink::default();
        kind.serialize(&mut bytes).expect("serialize u32 discriminant");
        assert_eq!(bytes.0, expected);
        let decoded = domain::Kind::deserialize(BinaryNumber::U32(u32::from_le_bytes(expected))).expect("deserialize u32 discriminant");
        assert_eq!(decoded, kind);
    }

    for (kind, expected) in [(domain::SmallKind::Tiny, 7), (domain::SmallKind::Large, 251)] {
        let mut bytes = BinarySink::default();
        kind.serialize(&mut bytes).expect("serialize u8 discriminant");
        assert_eq!(bytes.0, [expected]);
        let decoded = domain::SmallKind::deserialize(BinaryNumber::U8(expected)).expect("deserialize u8 discriminant");
        assert_eq!(decoded, kind);
    }
    for (kind, expected) in [
        (domain::MediumKind::Medium, [1, 2]),
        (domain::MediumKind::Maximum, [255, 255]),
    ] {
        let mut bytes = BinarySink::default();
        kind.serialize(&mut bytes).expect("serialize u16 discriminant");
        assert_eq!(bytes.0, expected);
        let decoded = domain::MediumKind::deserialize(BinaryNumber::U16(u16::from_le_bytes(expected)))
            .expect("deserialize u16 discriminant");
        assert_eq!(decoded, kind);
    }
    for (kind, expected) in [
        (domain::LargeKind::Large, [1, 0, 0, 0, 1, 0, 0, 0]),
        (domain::LargeKind::Maximum, [255; 8]),
    ] {
        let mut bytes = BinarySink::default();
        kind.serialize(&mut bytes).expect("serialize u64 discriminant");
        assert_eq!(bytes.0, expected);
        let decoded = domain::LargeKind::deserialize(BinaryNumber::U64(u64::from_le_bytes(expected)))
            .expect("deserialize u64 discriminant");
        assert_eq!(decoded, kind);
    }
    assert_eq!(domain::Kind::default(), domain::Kind::Empty);
    assert_eq!(domain::Value::default(), domain::Value::None);
    assert_eq!(
        serde_json::to_string(&domain::Kind::Empty).expect("serialize human JSON"),
        "\"none\""
    );
    assert_eq!(
        serde_json::from_str::<domain::Kind>("\"empty\"").expect("deserialize JSON alias"),
        domain::Kind::Empty
    );
    assert_eq!(
        serde_yaml::to_string(&domain::Kind::Empty).expect("serialize human YAML"),
        "none\n"
    );
    assert_eq!(
        serde_yaml::from_str::<domain::Kind>("empty\n").expect("deserialize YAML alias"),
        domain::Kind::Empty
    );
    assert_eq!(
        postcard::to_stdvec(&domain::Kind::Empty).expect("serialize postcard discriminant"),
        vec![17]
    );
    assert_eq!(
        postcard::from_bytes::<domain::Kind>(&[17]).expect("deserialize postcard discriminant"),
        domain::Kind::Empty
    );

    let empty_settings = Settings { values: Vec::new() };
    assert_eq!(
        serde_json::to_string(&empty_settings).expect("serialize empty JSON"),
        "{\"values\":null}"
    );
    let decoded_empty_json: Settings =
        serde_json::from_str("{\"values\":null}").expect("deserialize empty JSON");
    assert!(decoded_empty_json.values.is_empty());

    let populated_settings = Settings {
        values: vec![true, false],
    };
    assert_eq!(
        serde_json::to_string(&populated_settings).expect("serialize populated JSON"),
        "{\"values\":[true,false]}"
    );
    let decoded_populated_json: Settings =
        serde_json::from_str("{\"values\":[true,false]}").expect("deserialize populated JSON");
    assert_eq!(decoded_populated_json.values, [true, false]);
    assert_eq!(
        serde_yaml::to_string(&empty_settings).expect("serialize empty YAML"),
        "values: null\n"
    );
    let decoded_empty_yaml: Settings =
        serde_yaml::from_str("values: null\n").expect("deserialize empty YAML");
    assert!(decoded_empty_yaml.values.is_empty());
    assert_eq!(
        serde_yaml::to_string(&populated_settings).expect("serialize populated YAML"),
        "values:\n- true\n- false\n"
    );
    let decoded_populated_yaml: Settings =
        serde_yaml::from_str("values:\n- true\n- false\n").expect("deserialize populated YAML");
    assert_eq!(decoded_populated_yaml.values, [true, false]);
    assert_eq!(
        postcard::to_stdvec(&empty_settings).expect("serialize empty postcard"),
        vec![0]
    );
    assert_eq!(
        postcard::to_stdvec(&populated_settings).expect("serialize populated postcard"),
        vec![1, 2, 1, 0]
    );

    for settings in [empty_settings, populated_settings] {
        let bytes = postcard::to_stdvec(&settings).expect("serialize postcard");
        let decoded: Settings =
            postcard::from_bytes(&bytes).expect("deserialize postcard");
        assert_eq!(decoded.values, settings.values);
    }

    assert_eq!(generated_domain::Value::default(), domain::Value::None);
    assert_eq!(host_domain::Value::default(), domain::Value::None);

    let runtime = Arc::new(Runtime::builder().build().expect("build runtime"));
    let mut registration = generated::guest::init::register(
        Arc::clone(&runtime),
        generated::guest::interfaces::InternalProviders {
            provider_projection_value: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn generated_guest_contracts::ProjectionValueContract> { Box::new(Provider) }),
        },
    )
    .expect("register internal provider");
    for value in [
        domain::Value::None,
        domain::Value::Bool(true),
        domain::Value::Int(-47),
        domain::Value::Float(13.25),
        domain::Value::String("arena-string".to_owned()),
    ] {
        let result = registration
            .provider_projection_value
            .roundtrip(&value)
            .expect("host to guest tagged dispatch");
        assert_eq!(result, value);
    }

    let host = unsafe { HostContext::new(runtime.host_abi()) };
    let mut peer = generated::guest::peer_callers::ProjectionValueContractPeer::resolve(host)
        .expect("resolve projection peer provider");
    let scalar_input = domain::Value::Bool(true);
    let scalar = peer
        .roundtrip(&scalar_input)
        .expect("peer scalar tagged dispatch");
    assert!(matches!(scalar, domain::Value::Bool(true)));
    let string_input = domain::Value::String("arena-string".to_owned());
    let text = peer
        .roundtrip(&string_input)
        .expect("peer string tagged dispatch");
    match text {
        domain::Value::String(value) => assert_eq!(value, "arena-string"),
        _ => panic!("peer string dispatch must return the String projection"),
    }
    drop(peer);
    let bundle_id = registration.bundle_id;
    drop(registration);
    runtime.unload_bundle(bundle_id).expect("unload provider");
}
"#;
    fs::write(source_dir.join("main.rs"), consumer_source).expect("write consumer source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&crate_root)
        .output()
        .expect("run generated tagged projection consumer");
    assert!(
        output.status.success(),
        "generated tagged projection consumer failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let check = Command::new("cargo")
        .arg("check")
        .current_dir(&crate_root)
        .output()
        .expect("check generated tagged projection consumer");
    assert!(
        check.status.success(),
        "generated tagged projection consumer check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr),
    );
    let clippy = Command::new("cargo")
        .args(["clippy", "--", "-D", "warnings"])
        .current_dir(&crate_root)
        .output()
        .expect("lint generated tagged projection consumer");
    assert!(
        clippy.status.success(),
        "generated tagged projection consumer Clippy failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&clippy.stdout),
        String::from_utf8_lossy(&clippy.stderr),
    );
}

#[test]
fn generated_empty_internal_rust_bindings_compile_without_declaration_modules() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(&api, "").expect("write empty API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"empty_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n",
    )
    .expect("write empty internal bundle TOML");
    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate empty Rust internal bindings");
    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write binding-only output");
    let generated_root = Path::new("internal").join(format!(
        "empty_internal_plugin-{:016x}",
        bundle_id("empty_internal_plugin")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    let guest_mod = fs::read_to_string(source_dir.join(&generated_root).join("guest/mod.rs"))
        .expect("read generated guest module");
    assert!(
        !guest_mod.contains("pub mod domain;") && !guest_mod.contains("pub mod guest_contracts;"),
        "omitted declaration partitions must not leave unusable module declarations: {guest_mod}"
    );
    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"empty_generated_internal_plugin_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer Cargo.toml");
    fs::write(
        source_dir.join("main.rs"),
        format!("#[path = {generated_module_path:?}]\nmod generated;\nfn main() {{}}\n"),
    )
    .expect("write consumer source");
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&crate_root)
        .output()
        .expect("check binding-only consumer");
    assert!(
        output.status.success(),
        "generated empty Rust bindings did not compile without declarations:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_internal_rust_unified_and_split_semantic_derives_compile_with_abi_only_mirrors() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[types]]\nname = \"SemanticPayload\"\nlangs = { rust = { derives = [\"serde::Serialize\", \"serde::Deserialize\"], attributes = [\"allow(dead_code)\"] } }\nfields = [{ name = \"name\", type = \"StringView\" }, { name = \"payload\", type = \"Buffer\" }, { name = \"values\", type = \"Array<u32>\" }]\n\n[[types]]\nname = \"Inner\"\nfields = [{ name = \"payload\", type = \"Buffer\" }]\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"inner\", type = \"Inner\" }]\n\n[[guest_contract]]\nname = \"peer.buffer\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"echo\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write nested peer API TOML");
    let api_text = fs::read_to_string(&api).expect("read nested peer API TOML");
    fs::write(
        &api,
        format!(
            "{api_text}\n[[enum]]\nname = \"ValueTag\"\nrepr = \"u32\"\nlangs = {{ rust = {{ derives = [\"serde::Serialize\", \"serde::Deserialize\"] }} }}\n\n[[enum.variants]]\nname = \"Number\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Enabled\"\nvalue = \"2\"\n\n[[types]]\nname = \"Value\"\nlangs = {{ rust = {{ derives = [\"serde::Serialize\", \"serde::Deserialize\"], tagged_enum = {{ tag_field = \"tag\", variants = [{{ tag = \"Number\", name = \"Number\", payload = \"number\" }}, {{ tag = \"Enabled\", name = \"Enabled\", payload = \"enabled\" }}] }} }} }}\nfields = [{{ name = \"tag\", type = \"ValueTag\" }}, {{ name = \"number\", type = \"u32\" }}, {{ name = \"enabled\", type = \"bool\" }}]\n\n[[guest_contract.functions]]\nname = \"scalar\"\nparams = [{{ name = \"value\", type = \"Value\" }}]\nreturn = \"Value\"\n"
        ),
    )
    .expect("add scalar tagged projection");
    fs::write(
        &bundle,
        "[bundle]\nname = \"nested_peer_buffer\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[dependency]]\nkind = \"contract\"\ncontract = \"peer.buffer\"\nmin_version = \"1.0.0\"\n\n[[plugin]]\nname = \"provider\"\nimplements = [\"peer.buffer@1.0\"]\n",
    )
    .expect("write nested peer bundle TOML");
    let declarations_root = temp.path().join("declarations");
    let unified = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle.clone(),
        layout: OutputLayout::unified(),
    })
    .expect("generate unified semantic Rust bindings");

    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle.clone(),
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: declarations_root.clone(),
                import: ValidatedImport::parse(Lang::Rust, "crate::domain").expect("domain import"),
            },
            guest_contracts: OutputDestination::Emit {
                root: declarations_root.clone(),
                import: ValidatedImport::parse(Lang::Rust, "crate::guest_contracts")
                    .expect("guest-contract import"),
            },
        },
    })
    .expect("generate nested peer Rust bindings");
    let domain = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/domain.rs"))
        .expect("domain declarations")
        .content
        .as_str();
    assert!(
        domain.contains(
            "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\n#[allow(dead_code)]\npub struct SemanticPayload"
        )
            && domain.contains(
                "#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\npub enum ValueTag"
            )
            && domain.contains(
                "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\npub enum Value"
            ),
        "semantic derives must remain on ordinary and tagged domain projections: {domain}"
    );
    for abi_types in generated.files.iter().filter(|file| {
        file.path.ends_with("host/types.rs") || file.path.ends_with("guest/types.rs")
    }) {
        assert!(
            abi_types.content.contains(
                "#[repr(C)]\n#[allow(dead_code)]\n#[derive(Debug)]\npub struct SemanticPayload"
            ) && !abi_types.content.contains("serde::Serialize"),
            "ABI mirrors must retain mandatory derives and raw attributes without semantic derives: {}",
            abi_types.path.display()
        );
    }

    let peer_callers = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/peer_callers.rs"))
        .expect("peer caller output")
        .content
        .as_str();
    assert!(
        peer_callers.contains("Buffer { ptr:"),
        "nested Buffer peer conversion must construct an ABI Buffer: {peer_callers}"
    );
    assert!(
        peer_callers.contains("let host = self.host;"),
        "peer Buffer return cleanup must use the stored host field: {peer_callers}"
    );
    assert!(
        peer_callers.contains("number: *(payload)") && peer_callers.contains("enabled: *(payload)"),
        "split peer callers must dereference scalar projected payloads: {peer_callers}"
    );
    let host_callers = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("host/host_callers.rs"))
        .expect("host caller output")
        .content
        .as_str();
    assert!(
        host_callers.contains("number: *(payload)") && host_callers.contains("enabled: *(payload)"),
        "split host callers must dereference scalar projected payloads: {host_callers}"
    );

    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write nested peer bindings");
    let generated_root = Path::new("internal").join(format!(
        "nested_peer_buffer-{:016x}",
        bundle_id("nested_peer_buffer")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    let domain_module_path = declarations_root
        .join(&generated_root)
        .join("guest/domain.rs");
    let guest_contracts_module_path = declarations_root
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    let consumer_manifest = format!(
        "[package]\nname = \"nested_peer_buffer_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\n\n[workspace]\n",
        cargo_path("crates/polyplug"),
        cargo_path("crates/polyplug_abi"),
        cargo_path("crates/polyplug_common"),
        cargo_path("sdks/rust/guest"),
        cargo_path("crates/polyplug_utils"),
    );
    fs::write(crate_root.join("Cargo.toml"), &consumer_manifest)
        .expect("write split consumer Cargo.toml");
    let unified_root = temp.path().join("unified-consumer");
    let unified_source_dir = unified_root.join("src");
    fs::create_dir_all(&unified_source_dir).expect("create unified consumer source directory");
    write_output(&unified, &unified_source_dir).expect("write unified Rust bindings");
    let unified_module_path = unified_source_dir.join(&generated_root).join("mod.rs");
    fs::write(unified_root.join("Cargo.toml"), &consumer_manifest)
        .expect("write unified consumer Cargo.toml");
    fs::write(
        unified_source_dir.join("main.rs"),
        format!("#[path = {unified_module_path:?}]\nmod generated;\nfn main() {{}}\n"),
    )
    .expect("write unified consumer source");
    let unified_check = Command::new("cargo")
        .arg("check")
        .current_dir(&unified_root)
        .output()
        .expect("check unified semantic Rust bindings");
    assert!(
        unified_check.status.success(),
        "unified host and guest bindings with semantic derives did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&unified_check.stdout),
        String::from_utf8_lossy(&unified_check.stderr),
    );
    fs::write(
        source_dir.join("main.rs"),
        format!(
            "#[path = {domain_module_path:?}]\npub mod domain;\n#[path = {guest_contracts_module_path:?}]\npub mod guest_contracts;\n#[path = {generated_module_path:?}]\npub mod generated;\nfn main() {{}}\n"
        ),
    )
    .expect("write consumer source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&crate_root)
        .output()
        .expect("run nested peer consumer");
    assert!(
        output.status.success(),
        "generated nested peer Buffer caller did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let clippy = Command::new("cargo")
        .args(["clippy", "--", "-D", "warnings"])
        .current_dir(&crate_root)
        .output()
        .expect("lint split peer consumer");
    assert!(
        clippy.status.success(),
        "split host and peer caller consumer clippy failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&clippy.stdout),
        String::from_utf8_lossy(&clippy.stderr),
    );
}

#[test]
fn generated_ordinary_rust_guest_uses_external_domain_and_contract_paths() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    fs::write(
        &api,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Cold\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Hot\"\nvalue = \"2\"\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"mode\", type = \"Mode\" }]\n\n[[guest_contract]]\nname = \"demo.control\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"cycle\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write API TOML");
    let common = temp.path().join("common");
    let guest = temp.path().join("guest");
    fs::create_dir_all(common.join("src")).expect("create common source directory");
    fs::create_dir_all(guest.join("src")).expect("create guest source directory");
    fs::write(
        common.join("Cargo.toml"),
        format!(
            "[package]\nname = \"common\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug_guest = {{ path = \"{}\" }}\n",
            cargo_path("sdks/rust/guest"),
        ),
    )
    .expect("write common Cargo.toml");
    fs::write(
        common.join("src/lib.rs"),
        "pub mod domain {\n    #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n    pub enum Mode { Cold, Hot }\n    #[derive(Debug, Clone, PartialEq)]\n    pub struct Envelope { pub mode: Mode }\n}\npub mod guest_contracts {\n    use polyplug_guest::GuestError;\n    use super::domain::Envelope;\n    pub trait DemoControlContract: Send + Sync { fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError>; }\n}\n",
    )
    .expect("write common source");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::domain").expect("domain import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::guest_contracts")
                .expect("contract import"),
        },
    };
    let generated = generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::Rust,
        side: Side::Guest,
        layout,
    })
    .expect("generate split ordinary Rust guest");
    write_output(&generated, &guest.join("src/generated")).expect("write split guest bindings");
    let guest_mod =
        fs::read_to_string(guest.join("src/generated/guest/mod.rs")).expect("read guest module");
    assert!(
        guest_mod.contains("pub use common::domain;")
            && guest_mod.contains("pub use common::guest_contracts;"),
        "split guest must expose imported canonical declarations: {guest_mod}"
    );
    let interfaces = fs::read_to_string(guest.join("src/generated/guest/interfaces.rs"))
        .expect("read guest interfaces");
    assert!(
        interfaces.contains("Box<dyn common::guest_contracts::DemoControlContract>")
            && interfaces.contains("DemoControlDomainAdapter"),
        "guest factory and adapter must use the imported domain contract: {interfaces}"
    );
    fs::write(
        guest.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ordinary_split_guest\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug_abi"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write guest Cargo.toml");
    fs::write(
        guest.join("src/main.rs"),
        "#[path = \"generated/guest/mod.rs\"]\nmod generated;\n\nuse common::domain::{Envelope, Mode};\nuse common::guest_contracts::DemoControlContract;\nuse polyplug_guest::{GuestError, HostContext};\n\nstruct Provider;\nimpl DemoControlContract for Provider {\n    fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError> { Ok(Envelope { mode: match value.mode { Mode::Cold => Mode::Hot, Mode::Hot => Mode::Cold } }) }\n}\n#[unsafe(no_mangle)]\npub fn polyplug_create_demo_control(_host: HostContext) -> Box<dyn DemoControlContract> { Box::new(Provider) }\nfn main() { assert_eq!(Provider.cycle(Envelope { mode: Mode::Cold }).expect(\"cycle\").mode, Mode::Hot); }\n",
    )
    .expect("write guest source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&guest)
        .output()
        .expect("run split guest");
    assert!(
        output.status.success(),
        "ordinary split Rust guest did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let host = temp.path().join("host");
    fs::create_dir_all(host.join("src")).expect("create host source directory");
    let host_generated = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Rust,
        side: Side::Host,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::Rust, "common::domain")
                    .expect("domain import"),
            },
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate split ordinary Rust host");
    write_output(&host_generated, &host.join("src/generated")).expect("write split host bindings");
    let callers = fs::read_to_string(host.join("src/generated/host/host_callers.rs"))
        .expect("read host callers");
    assert!(
        callers.contains("value: &common::domain::Envelope")
            && callers.contains("Result<common::domain::Envelope, ContractError>"),
        "split host caller must expose canonical domain types: {callers}"
    );
    fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ordinary_split_host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
        ),
    )
    .expect("write host Cargo.toml");
    fs::write(
        host.join("src/main.rs"),
        "#[path = \"generated/mod.rs\"]\nmod generated;\n\nuse common::domain::{Envelope, Mode};\nuse generated::host::host_callers::ContractError;\nuse polyplug_abi::AbiErrorCode;\n\nfn assert_error<T: std::error::Error>() {}\n\nfn main() {\n    assert_error::<ContractError>();\n    let empty = ContractError::new(AbiErrorCode::Generic);\n    assert_eq!(empty.to_string(), \"ContractError(code=Generic, message=)\");\n    let detailed = ContractError { code: AbiErrorCode::NotFound, message: \"missing\".to_owned() };\n    assert_eq!(detailed.to_string(), \"ContractError(code=NotFound, message=missing)\");\n    assert_eq!(detailed.clone().code as u32, AbiErrorCode::NotFound as u32);\n    let value = Envelope { mode: Mode::Cold };\n    assert_eq!(value.mode, Mode::Cold);\n}\n",
    )
    .expect("write host source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&host)
        .output()
        .expect("run split host");
    assert!(
        output.status.success(),
        "ordinary split Rust host did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_internal_rust_three_crate_split_preserves_nominal_types_and_stateful_arrays() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Cold\"\nvalue = \"3\"\n\n[[enum.variants]]\nname = \"Warm\"\nvalue = \"7\"\n\n[[enum.variants]]\nname = \"Hot\"\nvalue = \"11\"\n\n[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\nbitflag = true\n\n[[enum.variants]]\nname = \"Read\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Write\"\nvalue = \"1 << 1\"\n\n[[enum.variants]]\nname = \"Read_Write\"\nvalue = \"Read | Write\"\n\n[[types]]\nname = \"Row\"\nfields = [{ name = \"modes\", type = \"Array<Mode>\" }]\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"mode\", type = \"Mode\" }, { name = \"flags\", type = \"Flags\" }, { name = \"text\", type = \"StringView\" }, { name = \"payload\", type = \"Buffer\" }, { name = \"rows\", type = \"Array<Row>\" }]\n\n[[guest_contract]]\nname = \"platform.plugin\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"cycle\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write split API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"split_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"platform\"\nimplements = [\"platform.plugin@1.0\"]\n",
    )
    .expect("write split bundle TOML");
    let generated_root = Path::new("internal").join(format!(
        "split_internal_plugin-{:016x}",
        bundle_id("split_internal_plugin")
    ));

    let common = temp.path().join("common");
    let platform = temp.path().join("platform");
    let core = temp.path().join("core");
    for crate_root in [&common, &platform, &core] {
        fs::create_dir_all(crate_root.join("src")).expect("create crate source directory");
    }
    fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"common\", \"platform\", \"core\"]\nresolver = \"3\"\n",
    )
    .expect("write temporary workspace manifest");

    let common_generated_root = common.join("src/generated");
    let common_layout = OutputLayout {
        bindings: OutputDestination::Omit,
        domain_types: OutputDestination::Emit {
            root: common_generated_root.clone(),
            import: ValidatedImport::parse(Lang::Rust, "crate::domain")
                .expect("common domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: common_generated_root.clone(),
            import: ValidatedImport::parse(Lang::Rust, "crate::guest_contracts")
                .expect("common guest contract import"),
        },
    };
    let common_output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle.clone(),
        layout: common_layout,
    })
    .expect("generate common declarations");
    write_output(&common_output, &common.join("src/ignored"))
        .expect("write common declaration partitions");
    let common_domain = common_generated_root
        .join(&generated_root)
        .join("guest/domain.rs");
    let common_contracts = common_generated_root
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    assert!(
        common_domain.is_file() && common_contracts.is_file(),
        "common must emit canonical domain and guest-contract declarations"
    );
    assert!(
        !common.join("src/ignored/guest/types.rs").exists(),
        "common must omit ABI binding partitions"
    );
    let common_domain_source =
        fs::read_to_string(&common_domain).expect("read generated common domain");
    assert_eq!(
        common_domain_source.matches("pub struct Envelope").count(),
        1,
        "common must own exactly one domain declaration"
    );
    fs::write(
        common.join("Cargo.toml"),
        format!(
            "[package]\nname = \"common\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug_guest = {{ path = \"{}\" }}\n",
            cargo_path("sdks/rust/guest"),
        ),
    )
    .expect("write common manifest");
    let common_domain_module_path = Path::new("generated")
        .join(&generated_root)
        .join("guest/domain.rs");
    let common_contracts_module_path = Path::new("generated")
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    fs::write(
        common.join("src/lib.rs"),
        format!(
            "#[path = {common_domain_module_path:?}]\npub mod domain;\n#[path = {common_contracts_module_path:?}]\npub mod guest_contracts;\n"
        ),
    )
    .expect("write common declarations module");

    fs::write(
        platform.join("Cargo.toml"),
        format!(
            "[package]\nname = \"platform\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\npolyplug_guest = {{ path = \"{}\" }}\n",
            cargo_path("sdks/rust/guest"),
        ),
    )
    .expect("write platform manifest");
    fs::write(
        platform.join("src/lib.rs"),
        "use std::sync::atomic::{AtomicUsize, Ordering};\n\nuse common::domain::{Envelope, Mode};\nuse common::guest_contracts::PlatformPluginContract;\nuse polyplug_guest::GuestError;\n\npub struct Platform {\n    calls: AtomicUsize,\n}\n\nimpl Platform {\n    pub fn new() -> Self {\n        Self { calls: AtomicUsize::new(0) }\n    }\n}\n\nimpl PlatformPluginContract for Platform {\n    fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError> {\n        let mode = if self.calls.fetch_add(1, Ordering::Relaxed) % 2 == 0 {\n            Mode::Hot\n        } else {\n            Mode::Warm\n        };\n        Ok(Envelope { mode, ..value })\n    }\n}\n"
    )
    .expect("write platform implementation");

    let core_generated_root = core.join("src/generated");
    let core_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::domain")
                .expect("core domain import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::guest_contracts")
                .expect("core guest-contract import"),
        },
    };
    let core_output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: core_layout,
    })
    .expect("generate core bindings");
    write_output(&core_output, &core_generated_root).expect("write core binding partition");
    assert!(
        !core_generated_root
            .join(&generated_root)
            .join("guest/domain.rs")
            .exists()
            && !core_generated_root
                .join(&generated_root)
                .join("guest/guest_contracts.rs")
                .exists(),
        "core must not emit duplicate declaration partitions"
    );
    let core_guest_mod = fs::read_to_string(
        core_generated_root
            .join(&generated_root)
            .join("guest/mod.rs"),
    )
    .expect("read generated core guest module");
    assert!(
        core_guest_mod.contains("pub use common::domain;")
            && core_guest_mod.contains("pub use common::guest_contracts;"),
        "core bindings must import canonical declarations: {core_guest_mod}"
    );
    fs::write(
        core.join("Cargo.toml"),
        format!(
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\nplatform = {{ path = \"../platform\" }}\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write core manifest");
    let generated_module_path = Path::new("generated").join(&generated_root).join("mod.rs");
    fs::write(
        core.join("src/main.rs"),
        generated_consumer_source(format!(
            "#[path = {generated_module_path:?}]\nmod generated;\n\nuse std::alloc::{{GlobalAlloc, Layout, System}};\nuse std::sync::Arc;\nuse std::sync::atomic::{{AtomicBool, AtomicUsize, Ordering}};\n\nuse common::domain::{{flags, Envelope, Mode, Row, INTERNAL_GENERATION_FINGERPRINT as DOMAIN_FINGERPRINT}};\nuse common::guest_contracts::INTERNAL_GENERATION_FINGERPRINT as CONTRACT_FINGERPRINT;\nuse platform::Platform;\nuse polyplug::Runtime;\n\nstatic BUFFER_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);\nstatic TRACK_BUFFER_ALLOCATIONS: AtomicBool = AtomicBool::new(false);\n\nstruct TrackingAllocator;\n\nunsafe impl GlobalAlloc for TrackingAllocator {{\n    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {{\n        let ptr = unsafe {{ System.alloc(layout) }};\n        if TRACK_BUFFER_ALLOCATIONS.load(Ordering::SeqCst) && layout.size() == 37 && !ptr.is_null() {{\n            BUFFER_ALLOCATIONS.fetch_add(1, Ordering::SeqCst);\n        }}\n        ptr\n    }}\n\n    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {{\n        if TRACK_BUFFER_ALLOCATIONS.load(Ordering::SeqCst) && layout.size() == 37 && !ptr.is_null() {{\n            BUFFER_ALLOCATIONS.fetch_sub(1, Ordering::SeqCst);\n        }}\n        unsafe {{ System.dealloc(ptr, layout) }};\n    }}\n}}\n\n#[global_allocator]\nstatic ALLOCATOR: TrackingAllocator = TrackingAllocator;\n\nfn main() {{\n    assert_eq!(DOMAIN_FINGERPRINT, CONTRACT_FINGERPRINT, \"common declarations must share one fingerprint\");\n    assert_eq!(DOMAIN_FINGERPRINT, generated::guest::interfaces::INTERNAL_GENERATION_FINGERPRINT, \"core bindings must retain the common declaration fingerprint\");\n\n    let runtime = Arc::new(Runtime::builder().build().expect(\"build runtime\"));\n    let mut registration = generated::guest::init::register(\n        Arc::clone(&runtime),\n        generated::guest::interfaces::InternalProviders {{\n            platform_platform_plugin: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn common::guest_contracts::PlatformPluginContract> {{ Box::new(Platform::new()) }}),\n        }},\n    )\n    .expect(\"register platform provider\");\n\n    let input = Envelope {{\n        mode: Mode::Cold,\n        flags: flags::READ_WRITE,\n        text: \"canonical common input\".to_owned(),\n        payload: vec![0xA5; 37],\n        rows: vec![\n            Row {{ modes: vec![Mode::Cold, Mode::Warm] }},\n            Row {{ modes: vec![Mode::Hot] }},\n        ],\n    }};\n    TRACK_BUFFER_ALLOCATIONS.store(true, Ordering::SeqCst);\n    let first = registration.platform_platform_plugin.cycle(&input).expect(\"first stateful roundtrip\");\n    assert_eq!(first.mode, Mode::Hot);\n    assert_eq!(first.flags, flags::READ_WRITE);\n    assert_eq!(first.text, input.text);\n    assert_eq!(first.payload, input.payload);\n    assert_eq!(first.rows, input.rows);\n    drop(first);\n    assert_eq!(BUFFER_ALLOCATIONS.load(Ordering::SeqCst), 0, \"returned Buffer must be copied and released through HostApi\");\n    let second = registration.platform_platform_plugin.cycle(&input).expect(\"preserve platform state\");\n    assert_eq!(second.mode, Mode::Warm);\n    assert_eq!(second.flags, flags::READ_WRITE);\n    assert_eq!(second.text, input.text);\n    assert_eq!(second.payload, input.payload);\n    assert_eq!(second.rows, input.rows);\n    drop(second);\n    assert_eq!(BUFFER_ALLOCATIONS.load(Ordering::SeqCst), 0, \"every returned Buffer allocation must be balanced\");\n    TRACK_BUFFER_ALLOCATIONS.store(false, Ordering::SeqCst);\n\n    let bundle_id = registration.bundle_id;\n    drop(registration);\n    runtime.unload_bundle(bundle_id).expect(\"unload after callers tear down\");\n}}\n"
        )),
    )
    .expect("write core executable");

    let output = Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg("core")
        .current_dir(temp.path())
        .output()
        .expect("run split internal Rust workspace");
    assert!(
        output.status.success(),
        "three-crate split internal Rust workspace did not compile, roundtrip, and unload:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let interfaces_path = core_generated_root
        .join(&generated_root)
        .join("guest/interfaces.rs");
    let interfaces = fs::read_to_string(&interfaces_path).expect("read core interface partition");
    let fingerprint_start = interfaces
        .find("pub const _POLYPLUG_INTERNAL_GENERATION_FINGERPRINT: u64 = ")
        .expect("interface fingerprint");
    let fingerprint_end = interfaces[fingerprint_start..]
        .find(';')
        .map(|offset| fingerprint_start + offset + 1)
        .expect("interface fingerprint terminator");
    let mut mismatched_interfaces = interfaces;
    mismatched_interfaces.replace_range(
        fingerprint_start..fingerprint_end,
        "pub const _POLYPLUG_INTERNAL_GENERATION_FINGERPRINT: u64 = 0;",
    );
    fs::write(&interfaces_path, mismatched_interfaces)
        .expect("write mismatched interface partition");
    let mismatch = Command::new("cargo")
        .arg("check")
        .arg("-p")
        .arg("core")
        .current_dir(temp.path())
        .output()
        .expect("compile mismatched split internal workspace");
    assert!(
        !mismatch.status.success(),
        "generated Rust root must reject mismatched declaration and binding partitions:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&mismatch.stdout),
        String::from_utf8_lossy(&mismatch.stderr),
    );
}
