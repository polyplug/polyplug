//! YAML configuration parser for SDK validator.
//!
//! Parses the golden method set configuration that defines the authoritative
//! method signatures each language SDK must implement. The yaml `naming:`
//! section is the source of truth for each language's naming convention, and
//! target paths resolve relative to the config file's parent directory.

use core::str::FromStr;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast_grep::NamingConvention;
use crate::error::ValidatorError;

/// The full set of languages the validator knows about.
pub const KNOWN_LANGUAGES: [&str; 6] = ["rust", "python", "csharp", "cpp", "js", "lua"];

/// Configuration for SDK validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Configuration format version (currently only 1).
    pub version: u32,
    /// Golden method set: struct name -> list of method names.
    pub methods: HashMap<String, Vec<String>>,
    /// Naming conventions: language -> parsed convention.
    pub naming: HashMap<String, NamingConvention>,
    /// Target SDK paths: language -> list of file paths, resolved relative to
    /// the config file's parent directory.
    pub targets: HashMap<String, Vec<PathBuf>>,
    /// Golden enum set: enum name -> variant name -> exact value.
    pub enums: HashMap<String, BTreeMap<String, i64>>,
    /// Enum mirror targets: language -> enum name -> list of file paths,
    /// resolved relative to the config file's parent directory.
    pub enum_targets: HashMap<String, HashMap<String, Vec<PathBuf>>>,
    /// Golden struct set: struct name -> ordered field names (declaration
    /// order is the ABI-layout proxy).
    pub structs: HashMap<String, Vec<String>>,
    /// Struct mirror targets: language -> struct name -> list of file paths,
    /// resolved relative to the config file's parent directory.
    pub struct_targets: HashMap<String, HashMap<String, Vec<PathBuf>>>,
}

/// Intermediate struct for YAML deserialization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct ConfigYaml {
    version: u32,
    methods: HashMap<String, Vec<String>>,
    naming: HashMap<String, String>,
    targets: HashMap<String, Vec<String>>,
    #[serde(default)]
    enums: HashMap<String, VariantEntriesYaml>,
    #[serde(default)]
    enum_targets: HashMap<String, HashMap<String, Vec<String>>>,
    #[serde(default)]
    structs: HashMap<String, Vec<String>>,
    #[serde(default)]
    struct_targets: HashMap<String, HashMap<String, Vec<String>>>,
}

/// Variant entries of one enum, preserving duplicates.
///
/// serde_yaml silently overwrites duplicate keys when deserializing into a
/// map, so the entries are captured as a list and duplicates rejected
/// explicitly in [`validate_enums`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct VariantEntriesYaml(Vec<(String, i64)>);

impl<'de> serde::Deserialize<'de> for VariantEntriesYaml {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EntriesVisitor;

        impl<'de> serde::de::Visitor<'de> for EntriesVisitor {
            type Value = VariantEntriesYaml;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a mapping of variant name to integer value")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut entries: Vec<(String, i64)> = Vec::new();
                while let Some((variant, value)) = map.next_entry::<String, i64>()? {
                    entries.push((variant, value));
                }
                Ok(VariantEntriesYaml(entries))
            }
        }

        deserializer.deserialize_map(EntriesVisitor)
    }
}

/// Parse and validate a YAML configuration file.
///
/// # Errors
///
/// Returns a [`ValidatorError`] if:
/// - the file cannot be read or is malformed YAML
/// - the version is not 1
/// - a struct lists a duplicate or non-identifier method name
/// - `targets:` or `naming:` contains an unknown language key
/// - a language present in `targets:` has no (or an invalid) `naming:` entry
pub fn parse_config(path: &Path) -> Result<Config, ValidatorError> {
    let content: String =
        std::fs::read_to_string(path).map_err(|source| ValidatorError::ConfigRead {
            path: path.to_path_buf(),
            source,
        })?;

    let yaml: ConfigYaml =
        serde_yaml::from_str(&content).map_err(|source| ValidatorError::ConfigParse {
            path: path.to_path_buf(),
            source,
        })?;

    if yaml.version != 1 {
        return Err(ValidatorError::UnsupportedConfigVersion {
            version: yaml.version,
        });
    }

    validate_methods(&yaml.methods)?;
    validate_language_keys("targets", yaml.targets.keys())?;
    validate_language_keys("naming", yaml.naming.keys())?;
    validate_language_keys("enum_targets", yaml.enum_targets.keys())?;
    validate_language_keys("struct_targets", yaml.struct_targets.keys())?;
    let enums: HashMap<String, BTreeMap<String, i64>> = validate_enums(&yaml.enums)?;
    validate_enum_targets(&enums, &yaml.enum_targets)?;
    validate_structs(&yaml.structs)?;
    validate_struct_targets(&yaml.structs, &yaml.struct_targets)?;

    let naming: HashMap<String, NamingConvention> = parse_naming(&yaml.naming, &yaml.targets)?;

    let base_dir: &Path = path.parent().unwrap_or_else(|| Path::new("."));
    let targets: HashMap<String, Vec<PathBuf>> = yaml
        .targets
        .into_iter()
        .map(|(language, files)| {
            let resolved: Vec<PathBuf> = files.iter().map(|f| base_dir.join(f)).collect();
            (language, resolved)
        })
        .collect();

    let enum_targets: HashMap<String, HashMap<String, Vec<PathBuf>>> = yaml
        .enum_targets
        .into_iter()
        .map(|(language, enums)| {
            let resolved: HashMap<String, Vec<PathBuf>> = enums
                .into_iter()
                .map(|(enum_name, files)| {
                    let paths: Vec<PathBuf> = files.iter().map(|f| base_dir.join(f)).collect();
                    (enum_name, paths)
                })
                .collect();
            (language, resolved)
        })
        .collect();

    let struct_targets: HashMap<String, HashMap<String, Vec<PathBuf>>> = yaml
        .struct_targets
        .into_iter()
        .map(|(language, structs)| {
            let resolved: HashMap<String, Vec<PathBuf>> = structs
                .into_iter()
                .map(|(struct_name, files)| {
                    let paths: Vec<PathBuf> = files.iter().map(|f| base_dir.join(f)).collect();
                    (struct_name, paths)
                })
                .collect();
            (language, resolved)
        })
        .collect();

    Ok(Config {
        version: yaml.version,
        methods: yaml.methods,
        naming,
        targets,
        enums,
        enum_targets,
        structs: yaml.structs,
        struct_targets,
    })
}

/// Check that a name is a plain ASCII identifier.
///
/// Method, enum, and variant names are interpolated into ast-grep rules, so
/// anything else would corrupt the rule.
fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// Reject non-identifier enum/variant names and duplicate variants, and
/// build the validated golden enum map.
fn validate_enums(
    enums: &HashMap<String, VariantEntriesYaml>,
) -> Result<HashMap<String, BTreeMap<String, i64>>, ValidatorError> {
    let mut validated: HashMap<String, BTreeMap<String, i64>> = HashMap::new();
    for (enum_name, entries) in enums {
        if !is_identifier(enum_name) {
            return Err(ValidatorError::InvalidEnumName {
                enum_name: enum_name.clone(),
            });
        }
        let mut variants: BTreeMap<String, i64> = BTreeMap::new();
        for (variant, value) in &entries.0 {
            if !is_identifier(variant) {
                return Err(ValidatorError::InvalidVariantName {
                    enum_name: enum_name.clone(),
                    variant: variant.clone(),
                });
            }
            if variants.insert(variant.clone(), *value).is_some() {
                return Err(ValidatorError::DuplicateVariant {
                    enum_name: enum_name.clone(),
                    variant: variant.clone(),
                });
            }
        }
        validated.insert(enum_name.clone(), variants);
    }
    Ok(validated)
}

/// Reject `enum_targets:` entries referencing enums absent from `enums:`.
fn validate_enum_targets(
    enums: &HashMap<String, BTreeMap<String, i64>>,
    enum_targets: &HashMap<String, HashMap<String, Vec<String>>>,
) -> Result<(), ValidatorError> {
    for (language, targets) in enum_targets {
        for enum_name in targets.keys() {
            if !enums.contains_key(enum_name) {
                return Err(ValidatorError::UnknownEnum {
                    language: language.clone(),
                    enum_name: enum_name.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Reject non-identifier struct/field names and duplicate fields.
///
/// Struct and field names are interpolated into ast-grep rules, so they must
/// be plain identifiers; declaration order in the `Vec` is preserved as the
/// ABI-layout proxy.
fn validate_structs(structs: &HashMap<String, Vec<String>>) -> Result<(), ValidatorError> {
    for (struct_name, field_list) in structs {
        if !is_identifier(struct_name) {
            return Err(ValidatorError::InvalidStructName {
                struct_name: struct_name.clone(),
            });
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for field in field_list {
            if !is_identifier(field) {
                return Err(ValidatorError::InvalidFieldName {
                    struct_name: struct_name.clone(),
                    field: field.clone(),
                });
            }
            if !seen.insert(field.as_str()) {
                return Err(ValidatorError::DuplicateField {
                    struct_name: struct_name.clone(),
                    field: field.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Reject `struct_targets:` entries referencing structs absent from `structs:`.
fn validate_struct_targets(
    structs: &HashMap<String, Vec<String>>,
    struct_targets: &HashMap<String, HashMap<String, Vec<String>>>,
) -> Result<(), ValidatorError> {
    for (language, targets) in struct_targets {
        for struct_name in targets.keys() {
            if !structs.contains_key(struct_name) {
                return Err(ValidatorError::UnknownStruct {
                    language: language.clone(),
                    struct_name: struct_name.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Reject duplicate and non-identifier method names.
///
/// Method names are interpolated into ast-grep rules, so they must be plain
/// snake_case identifiers.
fn validate_methods(methods: &HashMap<String, Vec<String>>) -> Result<(), ValidatorError> {
    for (struct_name, method_list) in methods {
        let mut seen: HashSet<&str> = HashSet::new();
        for method in method_list {
            if !seen.insert(method.as_str()) {
                return Err(ValidatorError::DuplicateMethod {
                    struct_name: struct_name.clone(),
                    method: method.clone(),
                });
            }
            if !is_identifier(method) {
                return Err(ValidatorError::InvalidMethodName {
                    struct_name: struct_name.clone(),
                    method: method.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Reject language keys outside [`KNOWN_LANGUAGES`].
fn validate_language_keys<'a>(
    section: &str,
    keys: impl Iterator<Item = &'a String>,
) -> Result<(), ValidatorError> {
    for key in keys {
        if !KNOWN_LANGUAGES.contains(&key.as_str()) {
            return Err(ValidatorError::UnknownLanguage {
                section: section.to_string(),
                language: key.clone(),
            });
        }
    }
    Ok(())
}

/// Parse `naming:` entries and require one for every language in `targets:`.
fn parse_naming(
    naming: &HashMap<String, String>,
    targets: &HashMap<String, Vec<String>>,
) -> Result<HashMap<String, NamingConvention>, ValidatorError> {
    let mut parsed: HashMap<String, NamingConvention> = HashMap::new();
    for (language, value) in naming {
        let convention: NamingConvention = NamingConvention::from_str(value).map_err(|_| {
            ValidatorError::InvalidNamingConvention {
                language: language.clone(),
                value: value.clone(),
            }
        })?;
        parsed.insert(language.clone(), convention);
    }

    for language in targets.keys() {
        if !parsed.contains_key(language) {
            return Err(ValidatorError::MissingNamingConvention {
                language: language.clone(),
            });
        }
    }

    Ok(parsed)
}

/// Restrict a config to a single struct (for the `--struct` CLI flag).
///
/// A `struct_name` of `None` returns the config unchanged; an unknown struct
/// name yields an empty method set.
pub fn filter_to_struct(config: &Config, struct_name: Option<&str>) -> Config {
    match struct_name {
        None => config.clone(),
        Some(name) => {
            let methods: HashMap<String, Vec<String>> = config
                .methods
                .get(name)
                .map(|m| {
                    let mut map: HashMap<String, Vec<String>> = HashMap::new();
                    map.insert(name.to_string(), m.clone());
                    map
                })
                .unwrap_or_default();

            let structs: HashMap<String, Vec<String>> = config
                .structs
                .get(name)
                .map(|fields| {
                    let mut map: HashMap<String, Vec<String>> = HashMap::new();
                    map.insert(name.to_string(), fields.clone());
                    map
                })
                .unwrap_or_default();

            Config {
                version: config.version,
                methods,
                naming: config.naming.clone(),
                targets: config.targets.clone(),
                enums: config.enums.clone(),
                enum_targets: config.enum_targets.clone(),
                structs,
                struct_targets: config.struct_targets.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config(content: &str) -> Result<NamedTempFile, Box<dyn core::error::Error>> {
        let mut file: NamedTempFile = NamedTempFile::new()?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    const VALID_YAML: &str = r#"
version: 1

methods:
  StringView:
    - to_str
    - starts_with
    - ends_with
    - strip_prefix
    - split

naming:
  rust: snake_case
  python: snake_case
  csharp: PascalCase
  js: camelCase
  cpp: snake_case
  lua: snake_case

targets:
  rust:
    - sdks/rust/guest/src/lib.rs
  python:
    - sdks/python/polyplug_abi/polyplug_abi/string_view_helper.py
"#;

    #[test]
    fn test_parse_config() -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_config(VALID_YAML)?;
        let config: Config = parse_config(file.path())?;

        assert_eq!(config.version, 1);
        let string_view_methods: &Vec<String> = config
            .methods
            .get("StringView")
            .ok_or("missing StringView methods")?;
        assert_eq!(string_view_methods.len(), 5);
        assert_eq!(config.naming.get("rust"), Some(&NamingConvention::Snake));
        assert_eq!(config.naming.get("csharp"), Some(&NamingConvention::Pascal));
        assert_eq!(config.naming.get("js"), Some(&NamingConvention::Camel));
        assert!(config.targets.contains_key("rust"));
        assert!(config.targets.contains_key("python"));
        Ok(())
    }

    #[test]
    fn test_parse_config_resolves_paths_relative_to_config_dir()
    -> Result<(), Box<dyn core::error::Error>> {
        let dir: tempfile::TempDir = tempfile::tempdir()?;
        let config_path: PathBuf = dir.path().join("cfg.yaml");
        std::fs::write(&config_path, VALID_YAML)?;

        let config: Config = parse_config(&config_path)?;
        let rust_targets: &Vec<PathBuf> =
            config.targets.get("rust").ok_or("missing rust targets")?;
        assert_eq!(
            rust_targets[0],
            dir.path().join("sdks/rust/guest/src/lib.rs")
        );
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_version() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String = VALID_YAML.replace("version: 1", "version: 2");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        assert!(matches!(
            parse_config(file.path()),
            Err(ValidatorError::UnsupportedConfigVersion { version: 2 })
        ));
        Ok(())
    }

    #[test]
    fn test_parse_config_duplicate_method() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  StringView:
    - to_str
    - to_str
naming:
  rust: snake_case
targets:
  rust:
    - src/lib.rs
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        assert!(matches!(
            parse_config(file.path()),
            Err(ValidatorError::DuplicateMethod { .. })
        ));
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_method_name() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  StringView:
    - "to_str; rm -rf"
naming:
  rust: snake_case
targets:
  rust:
    - src/lib.rs
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        assert!(matches!(
            parse_config(file.path()),
            Err(ValidatorError::InvalidMethodName { .. })
        ));
        Ok(())
    }

    #[test]
    fn test_parse_config_unknown_target_language() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  StringView:
    - to_str
naming:
  rust: snake_case
targets:
  rsut:
    - src/lib.rs
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::UnknownLanguage { section, language }) => {
                assert_eq!(section, "targets");
                assert_eq!(language, "rsut");
            }
            other => panic!("expected UnknownLanguage error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_unknown_naming_language() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  StringView:
    - to_str
naming:
  charp: PascalCase
targets: {}
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::UnknownLanguage { section, language }) => {
                assert_eq!(section, "naming");
                assert_eq!(language, "charp");
            }
            other => panic!("expected UnknownLanguage error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_missing_naming_for_target() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  StringView:
    - to_str
naming:
  rust: snake_case
targets:
  rust:
    - src/lib.rs
  lua:
    - src/abi.lua
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::MissingNamingConvention { language }) => {
                assert_eq!(language, "lua");
            }
            other => panic!("expected MissingNamingConvention error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_naming_value() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  StringView:
    - to_str
naming:
  rust: kebab-case
targets:
  rust:
    - src/lib.rs
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::InvalidNamingConvention { language, value }) => {
                assert_eq!(language, "rust");
                assert_eq!(value, "kebab-case");
            }
            other => panic!("expected InvalidNamingConvention error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_missing_file() {
        assert!(matches!(
            parse_config(Path::new("/nonexistent/config.yaml")),
            Err(ValidatorError::ConfigRead { .. })
        ));
    }

    #[test]
    fn test_parse_config_malformed_yaml() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: &str = r#"
version: 1
methods:
  - this is wrong
    - not a map
"#;
        let file: NamedTempFile = create_temp_config(yaml)?;
        assert!(matches!(
            parse_config(file.path()),
            Err(ValidatorError::ConfigParse { .. })
        ));
        Ok(())
    }

    fn create_test_config() -> Config {
        let mut methods: HashMap<String, Vec<String>> = HashMap::new();
        methods.insert(
            "StringView".to_string(),
            vec!["to_str".to_string(), "starts_with".to_string()],
        );
        methods.insert("Buffer".to_string(), vec!["as_slice".to_string()]);

        Config {
            version: 1,
            methods,
            naming: HashMap::new(),
            targets: HashMap::new(),
            enums: HashMap::new(),
            enum_targets: HashMap::new(),
            structs: HashMap::new(),
            struct_targets: HashMap::new(),
        }
    }

    #[test]
    fn test_filter_to_struct_none() {
        let config: Config = create_test_config();
        let filtered: Config = filter_to_struct(&config, None);
        assert_eq!(filtered.methods.len(), 2);
    }

    #[test]
    fn test_filter_to_struct_existing() {
        let config: Config = create_test_config();
        let filtered: Config = filter_to_struct(&config, Some("StringView"));
        assert_eq!(filtered.methods.len(), 1);
        assert!(filtered.methods.contains_key("StringView"));
    }

    #[test]
    fn test_filter_to_struct_nonexistent() {
        let config: Config = create_test_config();
        let filtered: Config = filter_to_struct(&config, Some("NonExistent"));
        assert!(filtered.methods.is_empty());
    }

    const VALID_ENUM_YAML: &str = r#"
version: 1
methods:
  StringView:
    - to_str
naming:
  rust: snake_case
  lua: snake_case
targets:
  rust:
    - sdks/rust/guest/src/lib.rs
enums:
  DispatchType:
    Native: 0
    VirtualMachine: 1
enum_targets:
  rust:
    DispatchType:
      - crates/polyplug_abi/src/dispatch/dispatch_type.rs
  lua:
    DispatchType:
      - sdks/lua/abi/abi.lua
"#;

    #[test]
    fn test_parse_config_enums() -> Result<(), Box<dyn core::error::Error>> {
        let dir: tempfile::TempDir = tempfile::tempdir()?;
        let config_path: PathBuf = dir.path().join("cfg.yaml");
        std::fs::write(&config_path, VALID_ENUM_YAML)?;

        let config: Config = parse_config(&config_path)?;
        let dispatch: &BTreeMap<String, i64> = config
            .enums
            .get("DispatchType")
            .ok_or("missing DispatchType golden enum")?;
        assert_eq!(dispatch.get("Native"), Some(&0));
        assert_eq!(dispatch.get("VirtualMachine"), Some(&1));

        let rust_targets: &HashMap<String, Vec<PathBuf>> = config
            .enum_targets
            .get("rust")
            .ok_or("missing rust enum targets")?;
        let files: &Vec<PathBuf> = rust_targets
            .get("DispatchType")
            .ok_or("missing DispatchType files")?;
        assert_eq!(
            files[0],
            dir.path()
                .join("crates/polyplug_abi/src/dispatch/dispatch_type.rs")
        );
        Ok(())
    }

    #[test]
    fn test_parse_config_missing_enum_sections_default_empty()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_config(VALID_YAML)?;
        let config: Config = parse_config(file.path())?;
        assert!(config.enums.is_empty());
        assert!(config.enum_targets.is_empty());
        Ok(())
    }

    #[test]
    fn test_parse_config_unknown_enum_target_language() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String = VALID_ENUM_YAML.replace("  lua:\n", "  lau:\n");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::UnknownLanguage { section, language }) => {
                assert_eq!(section, "enum_targets");
                assert_eq!(language, "lau");
            }
            other => panic!("expected UnknownLanguage error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_enum_target_references_unknown_enum()
    -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String =
            VALID_ENUM_YAML.replace("  lua:\n    DispatchType:", "  lua:\n    LogLevel:");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::UnknownEnum {
                language,
                enum_name,
            }) => {
                assert_eq!(language, "lua");
                assert_eq!(enum_name, "LogLevel");
            }
            other => panic!("expected UnknownEnum error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_duplicate_variant_is_fatal() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String = VALID_ENUM_YAML.replace("    Native: 0", "    Native: 0\n    Native: 7");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::DuplicateVariant { enum_name, variant }) => {
                assert_eq!(enum_name, "DispatchType");
                assert_eq!(variant, "Native");
            }
            other => panic!("expected DuplicateVariant error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_variant_name() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String = VALID_ENUM_YAML.replace("    Native: 0", "    \"Na tive\": 0");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::InvalidVariantName { enum_name, variant }) => {
                assert_eq!(enum_name, "DispatchType");
                assert_eq!(variant, "Na tive");
            }
            other => panic!("expected InvalidVariantName error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_enum_name() -> Result<(), Box<dyn core::error::Error>> {
        // Replace only the `enums:` key (2-space indent), not the
        // `enum_targets:` reference (4-space indent).
        let yaml: String = VALID_ENUM_YAML.replace("\n  DispatchType:", "\n  \"Dispatch Type\":");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::InvalidEnumName { enum_name }) => {
                assert_eq!(enum_name, "Dispatch Type");
            }
            other => panic!("expected InvalidEnumName error, got {other:?}"),
        }
        Ok(())
    }

    const VALID_STRUCT_YAML: &str = r#"
version: 1
methods:
  StringView:
    - to_str
naming:
  rust: snake_case
  lua: snake_case
targets:
  rust:
    - sdks/rust/guest/src/lib.rs
structs:
  StringView:
    - ptr
    - len
  AbiError:
    - code
    - message
struct_targets:
  rust:
    StringView:
      - crates/polyplug_abi/src/types/string_view.rs
  lua:
    StringView:
      - sdks/lua/abi/abi.lua
"#;

    #[test]
    fn test_parse_config_structs() -> Result<(), Box<dyn core::error::Error>> {
        let dir: tempfile::TempDir = tempfile::tempdir()?;
        let config_path: PathBuf = dir.path().join("cfg.yaml");
        std::fs::write(&config_path, VALID_STRUCT_YAML)?;

        let config: Config = parse_config(&config_path)?;
        let string_view: &Vec<String> = config
            .structs
            .get("StringView")
            .ok_or("missing StringView golden struct")?;
        assert_eq!(string_view, &vec!["ptr".to_string(), "len".to_string()]);

        let rust_targets: &HashMap<String, Vec<PathBuf>> = config
            .struct_targets
            .get("rust")
            .ok_or("missing rust struct targets")?;
        let files: &Vec<PathBuf> = rust_targets
            .get("StringView")
            .ok_or("missing StringView files")?;
        assert_eq!(
            files[0],
            dir.path()
                .join("crates/polyplug_abi/src/types/string_view.rs")
        );
        Ok(())
    }

    #[test]
    fn test_parse_config_missing_struct_sections_default_empty()
    -> Result<(), Box<dyn core::error::Error>> {
        let file: NamedTempFile = create_temp_config(VALID_YAML)?;
        let config: Config = parse_config(file.path())?;
        assert!(config.structs.is_empty());
        assert!(config.struct_targets.is_empty());
        Ok(())
    }

    #[test]
    fn test_parse_config_unknown_struct_target_language() -> Result<(), Box<dyn core::error::Error>>
    {
        let yaml: String = VALID_STRUCT_YAML.replace("  lua:\n", "  lau:\n");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::UnknownLanguage { section, language }) => {
                assert_eq!(section, "struct_targets");
                assert_eq!(language, "lau");
            }
            other => panic!("expected UnknownLanguage error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_struct_target_references_unknown_struct()
    -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String =
            VALID_STRUCT_YAML.replace("  lua:\n    StringView:", "  lua:\n    Version:");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::UnknownStruct {
                language,
                struct_name,
            }) => {
                assert_eq!(language, "lua");
                assert_eq!(struct_name, "Version");
            }
            other => panic!("expected UnknownStruct error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_duplicate_field_is_fatal() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String = VALID_STRUCT_YAML.replace("    - ptr\n", "    - ptr\n    - ptr\n");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::DuplicateField { struct_name, field }) => {
                assert_eq!(struct_name, "StringView");
                assert_eq!(field, "ptr");
            }
            other => panic!("expected DuplicateField error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_field_name() -> Result<(), Box<dyn core::error::Error>> {
        let yaml: String = VALID_STRUCT_YAML.replace("    - ptr", "    - \"p tr\"");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::InvalidFieldName { struct_name, field }) => {
                assert_eq!(struct_name, "StringView");
                assert_eq!(field, "p tr");
            }
            other => panic!("expected InvalidFieldName error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_config_invalid_struct_name() -> Result<(), Box<dyn core::error::Error>> {
        // `AbiError` appears only as a `structs:` key, so this rename is
        // unambiguous (unlike `StringView`, which is also a methods key).
        let yaml: String = VALID_STRUCT_YAML.replace("\n  AbiError:", "\n  \"Abi Error\":");
        let file: NamedTempFile = create_temp_config(&yaml)?;
        match parse_config(file.path()) {
            Err(ValidatorError::InvalidStructName { struct_name }) => {
                assert_eq!(struct_name, "Abi Error");
            }
            other => panic!("expected InvalidStructName error, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn test_filter_to_struct_carries_structs() {
        let mut structs: HashMap<String, Vec<String>> = HashMap::new();
        structs.insert(
            "StringView".to_string(),
            vec!["ptr".to_string(), "len".to_string()],
        );
        structs.insert("AbiError".to_string(), vec!["code".to_string()]);
        let config: Config = Config {
            version: 1,
            methods: HashMap::new(),
            naming: HashMap::new(),
            targets: HashMap::new(),
            enums: HashMap::new(),
            enum_targets: HashMap::new(),
            structs,
            struct_targets: HashMap::new(),
        };

        let filtered: Config = filter_to_struct(&config, Some("StringView"));
        assert_eq!(filtered.structs.len(), 1);
        assert!(filtered.structs.contains_key("StringView"));

        let none: Config = filter_to_struct(&config, None);
        assert_eq!(none.structs.len(), 2);
    }
}
