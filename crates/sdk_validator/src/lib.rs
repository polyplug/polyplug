//! SDK Validator - Cross-language SDK consistency validation tool.
//!
//! This crate provides functionality to validate that SDK implementations
//! across different languages (Rust, Python, C#, C++, TypeScript, JavaScript)
//! are consistent with each other.

pub mod aggregator;
pub mod ast_grep;
pub mod config;
pub mod languages;
pub mod reporter;

pub use aggregator::{
    LanguageReport, MethodStatus, StructReport, ValidationReport, aggregate_results,
};
pub use ast_grep::{
    AstGrepError, AstGrepRunner, Language, Match, NamingConvention, Range, generate_rule,
    transform_name,
};
pub use reporter::Reporter;
