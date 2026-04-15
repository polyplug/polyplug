/// Runtime type identifier — identifies the language/runtime hosting plugins.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLanguage {
    Rust = 0,
    Cpp = 1,
    Dotnet = 2,
    Python = 3,
    Lua = 4,
    JavaScript = 5,
}
