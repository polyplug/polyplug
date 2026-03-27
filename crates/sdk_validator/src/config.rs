//! YAML configuration parser for SDK validator.
//!
//! Parses the golden method set configuration that defines the authoritative
//! method signatures each language SDK must implement.

#![allow(dead_code)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Configuration for SDK validation.
///
/// This struct represents the YAML configuration file that defines:
/// - The golden method set (authoritative, NOT extracted from code)
/// - Naming conventions per language
/// - Target SDK file paths for each language
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Configuration format version (currently only 1)
    pub version: u32,
    /// Golden method set: struct name -> list of method names
    pub methods: HashMap<String, Vec<String>>,
    /// Naming conventions: language -> naming style (snake_case, PascalCase, camelCase)
    pub naming: HashMap<String, String>,
    /// Target SDK paths: language -> list of file paths
    pub targets: HashMap<String, Vec<String>>,
}

/// Intermediate struct for YAML deserialization.
///
/// This struct uses serde-derived deserialization with all fields optional
/// to provide better error messages for missing fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct ConfigYaml {
    version: u32,
    methods: HashMap<String, Vec<String>>,
    naming: HashMap<String, String>,
    targets: HashMap<String, Vec<String>>,
}

impl From<ConfigYaml> for Config {
    fn from(yaml: ConfigYaml) -> Self {
        Self {
            version: yaml.version,
            methods: yaml.methods,
            naming: yaml.naming,
            targets: yaml.targets,
        }
    }
}

/// Parse a YAML configuration file.
///
/// # Arguments
///
/// * `path` - Path to the YAML configuration file
///
/// # Returns
///
/// The parsed `Config` struct on success.
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be read
/// - The YAML is malformed
/// - Required fields are missing
/// - The version is not 1
pub fn parse_config(path: &Path) -> Result<Config> {
    let content: String = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let yaml: ConfigYaml = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML in: {}", path.display()))?;

    let config: Config = yaml.into();

    if config.version != 1 {
        anyhow::bail!(
            "Unsupported config version: {} (only version 1 is supported)",
            config.version
        );
    }

    for (struct_name, methods) in &config.methods {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for method in methods {
            if !seen.insert(method.as_str()) {
                anyhow::bail!(
                    "Duplicate method '{}' found in struct '{}'",
                    method,
                    struct_name
                );
            }
        }
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file: NamedTempFile = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[test]
    fn test_parse_config() {
        let yaml = r#"
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
    - crates/polyplug_guest/src/lib.rs
  python:
    - sdks/python/polyplug_abi/polyplug_abi/helpers.py
"#;

        let file: NamedTempFile = create_temp_config(yaml);
        let config: Config = parse_config(file.path()).expect("Failed to parse config");

        assert_eq!(config.version, 1);

        assert!(config.methods.contains_key("StringView"));
        let string_view_methods: &Vec<String> = config.methods.get("StringView").unwrap();
        assert_eq!(string_view_methods.len(), 5);
        assert!(string_view_methods.contains(&"to_str".to_string()));
        assert!(string_view_methods.contains(&"starts_with".to_string()));

        assert_eq!(config.naming.get("rust").unwrap(), "snake_case");
        assert_eq!(config.naming.get("csharp").unwrap(), "PascalCase");
        assert_eq!(config.naming.get("js").unwrap(), "camelCase");

        assert!(config.targets.contains_key("rust"));
        assert!(config.targets.contains_key("python"));
    }

    #[test]
    fn test_parse_config_multiple_structs() {
        let yaml = r#"
version: 1

methods:
  StringView:
    - to_str
    - starts_with
  BufferView:
    - as_slice
    - as_mut_slice

naming:
  rust: snake_case

targets:
  rust:
    - src/lib.rs
"#;

        let file: NamedTempFile = create_temp_config(yaml);
        let config: Config = parse_config(file.path()).expect("Failed to parse config");

        assert_eq!(config.methods.len(), 2);
        assert!(config.methods.contains_key("StringView"));
        assert!(config.methods.contains_key("BufferView"));
    }

    #[test]
    fn test_parse_config_invalid_version() {
        let yaml = r#"
version: 2

methods:
  StringView:
    - to_str

naming:
  rust: snake_case

targets:
  rust:
    - src/lib.rs
"#;

        let file: NamedTempFile = create_temp_config(yaml);
        let result: Result<Config> = parse_config(file.path());
        assert!(result.is_err());
        let err_msg: String = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported config version"));
    }

    #[test]
    fn test_parse_config_duplicate_method() {
        let yaml = r#"
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

        let file: NamedTempFile = create_temp_config(yaml);
        let result: Result<Config> = parse_config(file.path());
        assert!(result.is_err());
        let err_msg: String = result.unwrap_err().to_string();
        assert!(err_msg.contains("Duplicate method"));
        assert!(err_msg.contains("to_str"));
        assert!(err_msg.contains("StringView"));
    }

    #[test]
    fn test_parse_config_missing_file() {
        let result: Result<Config> = parse_config(Path::new("/nonexistent/config.yaml"));
        assert!(result.is_err());
        let err_msg: String = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to read config file"));
    }

    #[test]
    fn test_parse_config_malformed_yaml() {
        let yaml = r#"
version: 1
methods:
  - this is wrong
    - not a map
"#;

        let file: NamedTempFile = create_temp_config(yaml);
        let result: Result<Config> = parse_config(file.path());
        assert!(result.is_err());
        let err_msg: String = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to parse YAML"));
    }
}
