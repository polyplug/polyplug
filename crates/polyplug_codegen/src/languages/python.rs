//! Python code generator — produces Python ctypes bindings from ABI items.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// Python ABI code generator.
pub struct PythonGenerator;

impl PythonGenerator {
    pub fn new() -> Self {
        PythonGenerator
    }

    fn rust_type_to_python(rust_type: &str) -> String {
        if rust_type.starts_with('*') {
            if rust_type == "*const u8" {
                return String::from("ctypes.c_char_p");
            }
            return String::from("ctypes.c_void_p");
        }

        if rust_type.contains("c_void") {
            return String::from("ctypes.c_void_p");
        }

        match rust_type {
            "u64" => String::from("ctypes.c_uint64"),
            "u32" => String::from("ctypes.c_uint32"),
            "u16" => String::from("ctypes.c_uint16"),
            "u8" => String::from("ctypes.c_uint8"),
            "i64" => String::from("ctypes.c_int64"),
            "i32" => String::from("ctypes.c_int32"),
            "i16" => String::from("ctypes.c_int16"),
            "i8" => String::from("ctypes.c_int8"),
            "usize" => String::from("ctypes.c_size_t"),
            "isize" => String::from("ctypes.c_ssize_t"),
            "bool" => String::from("ctypes.c_bool"),
            "()" => String::from("None"),
            other => String::from(other),
        }
    }

    fn format_docstring(doc: &str, indent_level: usize) -> String {
        let indent: String = "    ".repeat(indent_level);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}\"\"\"{}\"\"\"\n", indent, lines[0])
        } else {
            let mut result: String = format!("{}\"\"\"{}\n", indent, lines[0]);
            for line in &lines[1..] {
                result.push_str(&format!("{}{}\n", indent, line));
            }
            result.push_str(&format!("{}\"\"\"\n", indent));
            result
        }
    }
}

impl CodeGenerator for PythonGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        format!("{}: int = {}\n", item.name, item.value)
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&item.name);
        output.push_str("(ctypes.Structure):\n");

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI struct.\"\"\"\n");
        }

        output.push_str("    _fields_ = [\n");
        for field in &item.fields {
            let py_type: String = Self::rust_type_to_python(&field.rust_type);
            output.push_str(&format!("        (\"{}\", {}),\n", field.name, py_type));
        }
        output.push_str("    ]\n");

        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&item.name);
        output.push_str("(enum.IntEnum):\n");

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI enum.\"\"\"\n");
        }

        for (i, variant) in item.variants.iter().enumerate() {
            if let Some(value) = variant.value {
                output.push_str(&format!("    {} = {}\n", variant.name, value));
            } else if i == 0 {
                output.push_str(&format!("    {} = 0\n", variant.name));
            } else {
                output.push_str(&format!("    {} = {}\n", variant.name, i));
            }
        }

        output
    }

    fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        output.push_str("\n\nclass ");
        output.push_str(&item.name);
        output.push_str("(ctypes.Union):\n");

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_docstring(doc, 1));
        } else {
            output.push_str("    \"\"\"ABI union.\"\"\"\n");
        }

        output.push_str("    _fields_ = [\n");
        for variant in &item.variants {
            let py_type: String = Self::rust_type_to_python(&variant.type_name);
            output.push_str(&format!("        (\"{}\", {}),\n", variant.name, py_type));
        }
        output.push_str("    ]\n");

        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let ret_type: String = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_python(t))
            .unwrap_or_else(|| "None".to_string());

        let params: String = item
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, Self::rust_type_to_python(&p.rust_type)))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "def {}({}) -> {}:\n    pass\n\n",
            item.name, params, ret_type
        )
    }

    fn file_extension(&self) -> &'static str {
        "py"
    }

    fn language_name(&self) -> &'static str {
        "python"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        "from __future__ import annotations\n\nimport ctypes\nimport enum\nfrom typing import ClassVar\n\n".to_string()
    }
}

impl Default for PythonGenerator {
    fn default() -> Self {
        Self::new()
    }
}
