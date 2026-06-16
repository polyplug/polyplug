//! Reserved-word detection across all six polyplug target languages.
//!
//! Identifiers in a contract (`.toml`) flow verbatim into generated source for
//! every target language. A name that is a keyword in any one of them produces
//! uncompilable output, so such names are rejected at parse time rather than
//! escaped/renamed per-generator. This module owns the union table.
//!
//! The table is intentionally a flat const list grouped by language section so
//! it is auditable; `reserved_in` reports *which* language(s) reserve a name so
//! the error message can point the author at the conflict.

/// A reserved keyword and the language that reserves it.
struct ReservedWord {
    /// The reserved identifier (exact, case-sensitive match).
    word: &'static str,
    /// Human-readable language label, e.g. "Rust", "C++", "polyplug".
    language: &'static str,
}

/// The union of reserved keywords across all six target languages plus the
/// polyplug-reserved names. Grouped by language section for auditability. A
/// single word may appear under several languages — `reserved_in` collects all
/// matching language labels.
const RESERVED_WORDS: &[ReservedWord] = &[
    // ── Rust ──────────────────────────────────────────────────────────────
    // The 2015 + 2018 keyword set, including reserved-for-future-use words.
    ReservedWord {
        word: "as",
        language: "Rust",
    },
    ReservedWord {
        word: "break",
        language: "Rust",
    },
    ReservedWord {
        word: "const",
        language: "Rust",
    },
    ReservedWord {
        word: "continue",
        language: "Rust",
    },
    ReservedWord {
        word: "crate",
        language: "Rust",
    },
    ReservedWord {
        word: "dyn",
        language: "Rust",
    },
    ReservedWord {
        word: "else",
        language: "Rust",
    },
    ReservedWord {
        word: "enum",
        language: "Rust",
    },
    ReservedWord {
        word: "extern",
        language: "Rust",
    },
    ReservedWord {
        word: "false",
        language: "Rust",
    },
    ReservedWord {
        word: "fn",
        language: "Rust",
    },
    ReservedWord {
        word: "for",
        language: "Rust",
    },
    ReservedWord {
        word: "if",
        language: "Rust",
    },
    ReservedWord {
        word: "impl",
        language: "Rust",
    },
    ReservedWord {
        word: "in",
        language: "Rust",
    },
    ReservedWord {
        word: "let",
        language: "Rust",
    },
    ReservedWord {
        word: "loop",
        language: "Rust",
    },
    ReservedWord {
        word: "match",
        language: "Rust",
    },
    ReservedWord {
        word: "mod",
        language: "Rust",
    },
    ReservedWord {
        word: "move",
        language: "Rust",
    },
    ReservedWord {
        word: "mut",
        language: "Rust",
    },
    ReservedWord {
        word: "pub",
        language: "Rust",
    },
    ReservedWord {
        word: "ref",
        language: "Rust",
    },
    ReservedWord {
        word: "return",
        language: "Rust",
    },
    ReservedWord {
        word: "self",
        language: "Rust",
    },
    ReservedWord {
        word: "Self",
        language: "Rust",
    },
    ReservedWord {
        word: "static",
        language: "Rust",
    },
    ReservedWord {
        word: "struct",
        language: "Rust",
    },
    ReservedWord {
        word: "super",
        language: "Rust",
    },
    ReservedWord {
        word: "trait",
        language: "Rust",
    },
    ReservedWord {
        word: "true",
        language: "Rust",
    },
    ReservedWord {
        word: "type",
        language: "Rust",
    },
    ReservedWord {
        word: "unsafe",
        language: "Rust",
    },
    ReservedWord {
        word: "use",
        language: "Rust",
    },
    ReservedWord {
        word: "where",
        language: "Rust",
    },
    ReservedWord {
        word: "while",
        language: "Rust",
    },
    ReservedWord {
        word: "async",
        language: "Rust",
    },
    ReservedWord {
        word: "await",
        language: "Rust",
    },
    ReservedWord {
        word: "abstract",
        language: "Rust",
    },
    ReservedWord {
        word: "become",
        language: "Rust",
    },
    ReservedWord {
        word: "box",
        language: "Rust",
    },
    ReservedWord {
        word: "do",
        language: "Rust",
    },
    ReservedWord {
        word: "final",
        language: "Rust",
    },
    ReservedWord {
        word: "macro",
        language: "Rust",
    },
    ReservedWord {
        word: "override",
        language: "Rust",
    },
    ReservedWord {
        word: "priv",
        language: "Rust",
    },
    ReservedWord {
        word: "typeof",
        language: "Rust",
    },
    ReservedWord {
        word: "unsized",
        language: "Rust",
    },
    ReservedWord {
        word: "virtual",
        language: "Rust",
    },
    ReservedWord {
        word: "yield",
        language: "Rust",
    },
    ReservedWord {
        word: "try",
        language: "Rust",
    },
    // ── C++ ───────────────────────────────────────────────────────────────
    // C++20 keyword set (subset overlapping with C identifiers). Words shared
    // with Rust (e.g. `const`, `static`, `struct`, `enum`) are listed once
    // under Rust above and need not be repeated; only C++-unique words appear.
    ReservedWord {
        word: "alignas",
        language: "C++",
    },
    ReservedWord {
        word: "alignof",
        language: "C++",
    },
    ReservedWord {
        word: "and",
        language: "C++",
    },
    ReservedWord {
        word: "and_eq",
        language: "C++",
    },
    ReservedWord {
        word: "asm",
        language: "C++",
    },
    ReservedWord {
        word: "auto",
        language: "C++",
    },
    ReservedWord {
        word: "bitand",
        language: "C++",
    },
    ReservedWord {
        word: "bitor",
        language: "C++",
    },
    ReservedWord {
        word: "bool",
        language: "C++",
    },
    ReservedWord {
        word: "case",
        language: "C++",
    },
    ReservedWord {
        word: "catch",
        language: "C++",
    },
    ReservedWord {
        word: "char",
        language: "C++",
    },
    ReservedWord {
        word: "char8_t",
        language: "C++",
    },
    ReservedWord {
        word: "char16_t",
        language: "C++",
    },
    ReservedWord {
        word: "char32_t",
        language: "C++",
    },
    ReservedWord {
        word: "class",
        language: "C++",
    },
    ReservedWord {
        word: "compl",
        language: "C++",
    },
    ReservedWord {
        word: "concept",
        language: "C++",
    },
    ReservedWord {
        word: "consteval",
        language: "C++",
    },
    ReservedWord {
        word: "constexpr",
        language: "C++",
    },
    ReservedWord {
        word: "constinit",
        language: "C++",
    },
    ReservedWord {
        word: "const_cast",
        language: "C++",
    },
    ReservedWord {
        word: "co_await",
        language: "C++",
    },
    ReservedWord {
        word: "co_return",
        language: "C++",
    },
    ReservedWord {
        word: "co_yield",
        language: "C++",
    },
    ReservedWord {
        word: "decltype",
        language: "C++",
    },
    ReservedWord {
        word: "default",
        language: "C++",
    },
    ReservedWord {
        word: "delete",
        language: "C++",
    },
    ReservedWord {
        word: "double",
        language: "C++",
    },
    ReservedWord {
        word: "dynamic_cast",
        language: "C++",
    },
    ReservedWord {
        word: "explicit",
        language: "C++",
    },
    ReservedWord {
        word: "export",
        language: "C++",
    },
    ReservedWord {
        word: "float",
        language: "C++",
    },
    ReservedWord {
        word: "friend",
        language: "C++",
    },
    ReservedWord {
        word: "goto",
        language: "C++",
    },
    ReservedWord {
        word: "inline",
        language: "C++",
    },
    ReservedWord {
        word: "int",
        language: "C++",
    },
    ReservedWord {
        word: "long",
        language: "C++",
    },
    ReservedWord {
        word: "mutable",
        language: "C++",
    },
    ReservedWord {
        word: "namespace",
        language: "C++",
    },
    ReservedWord {
        word: "new",
        language: "C++",
    },
    ReservedWord {
        word: "noexcept",
        language: "C++",
    },
    ReservedWord {
        word: "not",
        language: "C++",
    },
    ReservedWord {
        word: "not_eq",
        language: "C++",
    },
    ReservedWord {
        word: "nullptr",
        language: "C++",
    },
    ReservedWord {
        word: "operator",
        language: "C++",
    },
    ReservedWord {
        word: "or",
        language: "C++",
    },
    ReservedWord {
        word: "or_eq",
        language: "C++",
    },
    ReservedWord {
        word: "private",
        language: "C++",
    },
    ReservedWord {
        word: "protected",
        language: "C++",
    },
    ReservedWord {
        word: "public",
        language: "C++",
    },
    ReservedWord {
        word: "register",
        language: "C++",
    },
    ReservedWord {
        word: "reinterpret_cast",
        language: "C++",
    },
    ReservedWord {
        word: "requires",
        language: "C++",
    },
    ReservedWord {
        word: "short",
        language: "C++",
    },
    ReservedWord {
        word: "signed",
        language: "C++",
    },
    ReservedWord {
        word: "sizeof",
        language: "C++",
    },
    ReservedWord {
        word: "static_assert",
        language: "C++",
    },
    ReservedWord {
        word: "static_cast",
        language: "C++",
    },
    ReservedWord {
        word: "switch",
        language: "C++",
    },
    ReservedWord {
        word: "template",
        language: "C++",
    },
    ReservedWord {
        word: "this",
        language: "C++",
    },
    ReservedWord {
        word: "thread_local",
        language: "C++",
    },
    ReservedWord {
        word: "throw",
        language: "C++",
    },
    ReservedWord {
        word: "typedef",
        language: "C++",
    },
    ReservedWord {
        word: "typeid",
        language: "C++",
    },
    ReservedWord {
        word: "typename",
        language: "C++",
    },
    ReservedWord {
        word: "union",
        language: "C++",
    },
    ReservedWord {
        word: "unsigned",
        language: "C++",
    },
    ReservedWord {
        word: "using",
        language: "C++",
    },
    ReservedWord {
        word: "void",
        language: "C++",
    },
    ReservedWord {
        word: "volatile",
        language: "C++",
    },
    ReservedWord {
        word: "wchar_t",
        language: "C++",
    },
    ReservedWord {
        word: "xor",
        language: "C++",
    },
    ReservedWord {
        word: "xor_eq",
        language: "C++",
    },
    // ── C# ────────────────────────────────────────────────────────────────
    // C# reserved keywords. Words shared with Rust/C++ above are not repeated;
    // only C#-unique reserved words appear here.
    ReservedWord {
        word: "base",
        language: "C#",
    },
    ReservedWord {
        word: "byte",
        language: "C#",
    },
    ReservedWord {
        word: "checked",
        language: "C#",
    },
    ReservedWord {
        word: "decimal",
        language: "C#",
    },
    ReservedWord {
        word: "delegate",
        language: "C#",
    },
    ReservedWord {
        word: "event",
        language: "C#",
    },
    ReservedWord {
        word: "finally",
        language: "C#",
    },
    ReservedWord {
        word: "fixed",
        language: "C#",
    },
    ReservedWord {
        word: "foreach",
        language: "C#",
    },
    ReservedWord {
        word: "implicit",
        language: "C#",
    },
    ReservedWord {
        word: "interface",
        language: "C#",
    },
    ReservedWord {
        word: "internal",
        language: "C#",
    },
    ReservedWord {
        word: "is",
        language: "C#",
    },
    ReservedWord {
        word: "lock",
        language: "C#",
    },
    ReservedWord {
        word: "null",
        language: "C#",
    },
    ReservedWord {
        word: "object",
        language: "C#",
    },
    ReservedWord {
        word: "out",
        language: "C#",
    },
    ReservedWord {
        word: "params",
        language: "C#",
    },
    ReservedWord {
        word: "readonly",
        language: "C#",
    },
    ReservedWord {
        word: "sbyte",
        language: "C#",
    },
    ReservedWord {
        word: "sealed",
        language: "C#",
    },
    ReservedWord {
        word: "stackalloc",
        language: "C#",
    },
    ReservedWord {
        word: "string",
        language: "C#",
    },
    ReservedWord {
        word: "uint",
        language: "C#",
    },
    ReservedWord {
        word: "ulong",
        language: "C#",
    },
    ReservedWord {
        word: "unchecked",
        language: "C#",
    },
    ReservedWord {
        word: "ushort",
        language: "C#",
    },
    // ── Python ────────────────────────────────────────────────────────────
    // Python keyword set. Words shared above are not repeated.
    ReservedWord {
        word: "and",
        language: "Python",
    },
    ReservedWord {
        word: "assert",
        language: "Python",
    },
    ReservedWord {
        word: "class",
        language: "Python",
    },
    ReservedWord {
        word: "def",
        language: "Python",
    },
    ReservedWord {
        word: "del",
        language: "Python",
    },
    ReservedWord {
        word: "elif",
        language: "Python",
    },
    ReservedWord {
        word: "except",
        language: "Python",
    },
    ReservedWord {
        word: "from",
        language: "Python",
    },
    ReservedWord {
        word: "global",
        language: "Python",
    },
    ReservedWord {
        word: "import",
        language: "Python",
    },
    ReservedWord {
        word: "is",
        language: "Python",
    },
    ReservedWord {
        word: "lambda",
        language: "Python",
    },
    ReservedWord {
        word: "nonlocal",
        language: "Python",
    },
    ReservedWord {
        word: "None",
        language: "Python",
    },
    ReservedWord {
        word: "not",
        language: "Python",
    },
    ReservedWord {
        word: "or",
        language: "Python",
    },
    ReservedWord {
        word: "pass",
        language: "Python",
    },
    ReservedWord {
        word: "raise",
        language: "Python",
    },
    ReservedWord {
        word: "True",
        language: "Python",
    },
    ReservedWord {
        word: "False",
        language: "Python",
    },
    ReservedWord {
        word: "with",
        language: "Python",
    },
    // ── Lua ───────────────────────────────────────────────────────────────
    // Lua 5.x keyword set. Words shared above are not repeated.
    ReservedWord {
        word: "elseif",
        language: "Lua",
    },
    ReservedWord {
        word: "end",
        language: "Lua",
    },
    ReservedWord {
        word: "function",
        language: "Lua",
    },
    ReservedWord {
        word: "local",
        language: "Lua",
    },
    ReservedWord {
        word: "nil",
        language: "Lua",
    },
    ReservedWord {
        word: "repeat",
        language: "Lua",
    },
    ReservedWord {
        word: "then",
        language: "Lua",
    },
    ReservedWord {
        word: "until",
        language: "Lua",
    },
    // ── JavaScript ────────────────────────────────────────────────────────
    // ECMAScript reserved words (incl. strict-mode + future-reserved). Words
    // shared above are not repeated.
    ReservedWord {
        word: "debugger",
        language: "JavaScript",
    },
    ReservedWord {
        word: "function",
        language: "JavaScript",
    },
    ReservedWord {
        word: "instanceof",
        language: "JavaScript",
    },
    ReservedWord {
        word: "let",
        language: "JavaScript",
    },
    ReservedWord {
        word: "var",
        language: "JavaScript",
    },
    ReservedWord {
        word: "with",
        language: "JavaScript",
    },
    ReservedWord {
        word: "yield",
        language: "JavaScript",
    },
    ReservedWord {
        word: "arguments",
        language: "JavaScript",
    },
    ReservedWord {
        word: "eval",
        language: "JavaScript",
    },
    // ── polyplug ──────────────────────────────────────────────────────────
    // Names the generators emit themselves; a user identifier matching one of
    // these would shadow or collide with generated symbols. The `polyplug_` and
    // `_polyplug` prefixes are handled separately in `reserved_in` because they
    // are a prefix family, not exact words.
    ReservedWord {
        word: "polyplug_init",
        language: "polyplug",
    },
];

/// Prefixes the polyplug generators reserve for their own emitted symbols.
/// Any user identifier starting with one of these would collide with generated
/// code (e.g. `polyplug_init`, `polyplug_create_<plugin>`).
const RESERVED_PREFIXES: &[&str] = &["polyplug_", "_polyplug"];

/// Return the human-readable list of language(s) that reserve `name`, or `None`
/// if the name is safe in every target language. Languages are returned in
/// table order with duplicates removed, e.g. `Some("C++, Python")`.
///
/// A name is reserved if it exactly matches a keyword in the table, or if it
/// begins with a polyplug-reserved prefix.
pub fn reserved_in(name: &str) -> Option<String> {
    let mut langs: Vec<&'static str> = Vec::new();

    for entry in RESERVED_WORDS {
        if entry.word == name && !langs.contains(&entry.language) {
            langs.push(entry.language);
        }
    }

    for prefix in RESERVED_PREFIXES {
        if name.starts_with(prefix) {
            let label: &'static str = "polyplug";
            if !langs.contains(&label) {
                langs.push(label);
            }
            break;
        }
    }

    if langs.is_empty() {
        None
    } else {
        Some(langs.join(", "))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::reserved_in;

    #[test]
    fn class_is_reserved_in_python_and_cpp() {
        let langs: String = reserved_in("class").expect("`class` must be reserved");
        assert!(langs.contains("C++"), "got: {langs}");
        assert!(langs.contains("Python"), "got: {langs}");
    }

    #[test]
    fn end_is_reserved_in_lua() {
        let langs: String = reserved_in("end").expect("`end` must be reserved");
        assert!(langs.contains("Lua"), "got: {langs}");
    }

    #[test]
    fn def_is_reserved_in_python() {
        let langs: String = reserved_in("def").expect("`def` must be reserved");
        assert!(langs.contains("Python"), "got: {langs}");
    }

    #[test]
    fn fn_is_reserved_in_rust() {
        let langs: String = reserved_in("fn").expect("`fn` must be reserved");
        assert!(langs.contains("Rust"), "got: {langs}");
    }

    #[test]
    fn int_is_reserved_in_cpp() {
        let langs: String = reserved_in("int").expect("`int` must be reserved");
        assert!(langs.contains("C++"), "got: {langs}");
    }

    #[test]
    fn ref_is_reserved_in_rust() {
        let langs: String = reserved_in("ref").expect("`ref` must be reserved");
        assert!(langs.contains("Rust"), "got: {langs}");
    }

    #[test]
    fn await_is_reserved() {
        assert!(reserved_in("await").is_some());
    }

    #[test]
    fn polyplug_init_is_reserved() {
        let langs: String = reserved_in("polyplug_init").expect("must be reserved");
        assert!(langs.contains("polyplug"), "got: {langs}");
    }

    #[test]
    fn polyplug_prefix_is_reserved() {
        assert!(reserved_in("polyplug_anything").is_some());
        assert!(reserved_in("_polyplug_handlers").is_some());
    }

    #[test]
    fn normal_names_are_not_reserved() {
        assert!(reserved_in("decode").is_none());
        assert!(reserved_in("transform").is_none());
        assert!(reserved_in("log_with_level").is_none());
        assert!(reserved_in("LogLevel").is_none());
        assert!(reserved_in("Debug").is_none());
        assert!(reserved_in("my_field").is_none());
    }
}
