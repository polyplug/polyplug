pub mod error;
pub mod generators;
pub mod ir;
pub mod pack;
pub mod parser;

use std::fs;
use std::path::PathBuf;

pub use error::PolyplugcError;
pub use ir::{
    EnumDef, EnumVariant, ReprType, ResolvedBundle, ResolvedContract, ResolvedDependency,
    ResolvedField, ResolvedFunction, ResolvedParam, ResolvedPlugin, ResolvedType, ResolvedTypeRef,
    ValidatedIr, Version,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Cpp,
    CSharp,
    Python,
    Lua,
    JsQuickJs,
    JsDeno,
}

impl Lang {
    fn as_str(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Cpp => "cpp",
            Lang::CSharp => "csharp",
            Lang::Python => "python",
            Lang::Lua => "lua",
            Lang::JsQuickJs => "js-quickjs",
            Lang::JsDeno => "js-deno",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Host,
    Guest,
}

#[derive(Debug)]
pub struct GenerateConfig {
    pub api_toml: PathBuf,
    pub lang: Lang,
    pub side: Side,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug)]
pub struct GenerateOutput {
    pub files: Vec<GeneratedFile>,
}

#[derive(Debug)]
pub struct PackConfig {
    pub manifest: PathBuf,
    pub lang: Lang,
    pub out_dir: PathBuf,
}

pub fn generate(config: GenerateConfig) -> Result<GenerateOutput, PolyplugcError> {
    use crate::generators::{
        CodeGenerator, GeneratedFile as InternalGeneratedFile, GeneratedFiles,
    };

    let file_content: String =
        fs::read_to_string(&config.api_toml).map_err(|e: std::io::Error| {
            PolyplugcError::ReadFailed {
                path: config.api_toml.to_string_lossy().to_string(),
                source: e,
            }
        })?;
    let ir: ValidatedIr = if file_content.contains("[bundle]") {
        parser::parse_bundle_with_api(&config.api_toml)?
    } else {
        parser::parse_api(&config.api_toml)?
    };

    let generator: Box<dyn CodeGenerator> = match config.lang {
        Lang::Rust => Box::new(generators::rust::RustGenerator),
        Lang::Cpp => Box::new(generators::cpp::CppGenerator),
        Lang::CSharp => Box::new(generators::csharp::CSharpGenerator),
        Lang::Python => Box::new(generators::python::PythonGenerator),
        Lang::Lua => Box::new(generators::lua::LuaGenerator),
        Lang::JsQuickJs => Box::new(generators::js_quickjs::JsQuickjsGenerator),
        Lang::JsDeno => Box::new(generators::js_deno::JsDenoGenerator),
    };

    let mut files: GeneratedFiles = GeneratedFiles::default();
    match config.side {
        Side::Host => generator.generate_host(&ir, &mut files)?,
        Side::Guest => generator.generate_guest(&ir, &mut files)?,
    }

    let public_files: Vec<GeneratedFile> = files
        .files
        .into_iter()
        .map(|f: InternalGeneratedFile| GeneratedFile {
            path: f.path,
            content: f.content,
        })
        .collect();

    Ok(GenerateOutput {
        files: public_files,
    })
}

pub fn pack(config: PackConfig) -> Result<(), PolyplugcError> {
    let ir: ValidatedIr = if config.manifest.ends_with("bundle.toml") {
        parser::parse_bundle_with_api(&config.manifest)?
    } else {
        parser::parse_api(&config.manifest)?
    };

    pack::run(&ir, &config.out_dir, config.lang.as_str())?;

    Ok(())
}
