//! YAML configuration parser for SDK validator.
//!
//! Parses the golden method set configuration that defines the authoritative
//! method signatures each language SDK must implement. The yaml `naming:`
//! section is the source of truth for each language's naming convention, and
//! target paths resolve relative to the config file's parent directory.

use core::str::FromStr;
use std::collections::{HashMap, HashSet};
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
}

/// Intermediate struct for YAML deserialization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct ConfigYaml {
    version: u32,
    methods: HashMap<String, Vec<String>>,
    naming: HashMap<String, String>,
    targets: HashMap<String, Vec<String>>,
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

    Ok(Config {
        version: yaml.version,
        methods: yaml.methods,
        naming,
        targets,
    })
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
            let is_identifier: bool = !method.is_empty()
                && method
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !method.starts_with(|c: char| c.is_ascii_digit());
            if !is_identifier {
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

            Config {
                version: config.version,
                methods,
                naming: config.naming.clone(),
                targets: config.targets.clone(),
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
}
