pub mod generators;
pub mod ir;
pub mod parser;
pub mod validate;

use std::fs;

use crate::ir::ValidatedIr;
use generators::{CodeGenerator, GeneratedFile as InternalGeneratedFile, GeneratedFiles};
use polyplug_codegen::{GenerateConfig, GenerateOutput, Lang, PolyplugcError, Side};

pub fn generate(config: GenerateConfig) -> Result<GenerateOutput, PolyplugcError> {
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
    };

    let mut files: GeneratedFiles = GeneratedFiles::default();
    match config.side {
        Side::Host => generator.generate_host(&ir, &mut files)?,
        Side::Guest => generator.generate_guest(&ir, &mut files)?,
    }

    let public_files: Vec<polyplug_codegen::GeneratedFile> = files
        .files
        .into_iter()
        .map(|f: InternalGeneratedFile| polyplug_codegen::GeneratedFile {
            path: f.path,
            content: f.content,
        })
        .collect();

    Ok(GenerateOutput {
        files: public_files,
    })
}

pub fn parse_lang(lang: &str) -> Result<Lang, PolyplugcError> {
    match lang {
        "rust" => Ok(Lang::Rust),
        "cpp" | "c++" => Ok(Lang::Cpp),
        "csharp" | "c#" => Ok(Lang::CSharp),
        "python" | "py" => Ok(Lang::Python),
        "lua" => Ok(Lang::Lua),
        "js-quickjs" => Ok(Lang::JsQuickJs),
        other => Err(PolyplugcError::ValidationFailed {
            message: format!(
                "Unknown language: `{other}`. Supported: rust, cpp, csharp, python, lua, js-quickjs"
            ),
        }),
    }
}
