mod aggregator;
mod ast_grep;
mod cli;
mod config;
mod languages;
mod reporter;

use std::collections::HashMap;
use std::process::ExitCode;

use anyhow::Result;

use crate::aggregator::aggregate_results;
use crate::ast_grep::AstGrepRunner;
use crate::cli::Args;
use crate::config::{Config, parse_config};
use crate::reporter::Reporter;

fn main() -> ExitCode {
    let args: Args = Args::parse_args();

    match run(args) {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Args) -> Result<ExitCode> {
    let config: Config = parse_config(&args.config)?;
    let filtered_config: Config = filter_config(&config, args.struct_name.as_deref());
    let runner: AstGrepRunner = AstGrepRunner::new();
    let report = aggregate_results(&filtered_config, &runner);
    let reporter: Reporter = Reporter::new();

    let output: String = if args.json {
        reporter.generate_json(&report)
    } else {
        reporter.generate_table(&report)
    };

    println!("{output}");

    if args.fail_on_missing && !report.is_complete {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn filter_config(config: &Config, struct_name: Option<&str>) -> Config {
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
    fn test_filter_config_none() {
        let config: Config = create_test_config();
        let filtered: Config = filter_config(&config, None);

        assert_eq!(filtered.methods.len(), 2);
    }

    #[test]
    fn test_filter_config_existing_struct() {
        let config: Config = create_test_config();
        let filtered: Config = filter_config(&config, Some("StringView"));

        assert_eq!(filtered.methods.len(), 1);
        assert!(filtered.methods.contains_key("StringView"));
        assert!(!filtered.methods.contains_key("Buffer"));
    }

    #[test]
    fn test_filter_config_nonexistent_struct() {
        let config: Config = create_test_config();
        let filtered: Config = filter_config(&config, Some("NonExistent"));

        assert!(filtered.methods.is_empty());
    }
}
