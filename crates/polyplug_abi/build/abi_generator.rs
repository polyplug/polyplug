use std::path::PathBuf;

use crate::{GeneratedFile, GeneratedFiles, abi_type_info::AbiInfo};

/// Trait for language-specific ABI code generators.
///
/// Each target language (C#, Python, Lua, etc.) implements this trait to
/// generate idiomatic bindings for the polyplug ABI types.
///
/// # Example Implementation
///
/// ```rust,ignore
/// struct CSharpGenerator;
///
/// impl AbiGenerator for CSharpGenerator {
///     fn generate_constants(&self, info: &AbiInfo) -> String {
///         // Generate C# constant definitions
///     }
///
///     fn generate_structs(&self, info: &AbiInfo) -> String {
///         // Generate C# struct definitions
///     }
///
///     // ... other methods
/// }
/// ```
pub trait AbiGenerator {
    /// Generate constant definitions for the target language.
    ///
    /// This includes ABI version, error codes, and other constants.
    fn generate_constants(&self, info: &AbiInfo) -> String;

    /// Generate struct definitions for the target language.
    ///
    /// This includes all `#[repr(C)]` structs from the ABI:
    /// StringView, Buffer, AbiError, PluginHandle, HostContext, etc.
    fn generate_structs(&self, info: &AbiInfo) -> String;

    /// Generate enum definitions for the target language.
    ///
    /// This includes DispatchType and any other C-style enums.
    fn generate_enums(&self, info: &AbiInfo) -> String;

    /// Generate union definitions for the target language.
    ///
    /// This includes PluginDispatch and any other unions.
    fn generate_unions(&self, info: &AbiInfo) -> String;

    /// Generate helper functions for the target language.
    ///
    /// This includes FNV-1a hash implementations and string helpers.
    fn generate_helpers(&self, info: &AbiInfo) -> String;

    /// Return the file extension for this language (e.g., "cs", "py", "lua").
    fn file_extension(&self) -> &'static str;

    /// Return the output directory name for this language (e.g., "csharp", "python").
    fn output_dir(&self) -> &'static str;

    /// Generate all ABI bindings and return the collection of files.
    ///
    /// The default implementation calls each generate_* method and combines
    /// the results into a single file. Implementations may override this
    /// to produce multiple files.
    fn generate(&self, info: &AbiInfo) -> GeneratedFiles {
        let mut files: GeneratedFiles = GeneratedFiles::new();

        let mut content: String = String::new();
        content.push_str(&self.generate_constants(info));
        content.push_str(&self.generate_structs(info));
        content.push_str(&self.generate_enums(info));
        content.push_str(&self.generate_unions(info));
        content.push_str(&self.generate_helpers(info));

        let filename: String = format!("abi.{}", self.file_extension());
        files.push(GeneratedFile {
            path: PathBuf::from(filename),
            content,
        });

        files
    }
}
