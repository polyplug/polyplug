//! JavaScript/TypeScript code generator — produces TypeScript types from ABI items.

use crate::data::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, UnionInfo};
use crate::languages::{CodeGenerator, GenerationContext};

/// JavaScript/TypeScript ABI code generator.
pub struct JsGenerator;

impl JsGenerator {
    pub fn new() -> Self {
        JsGenerator
    }

    fn rust_type_to_ts(rust_type: &str) -> String {
        if rust_type.contains("extern\"C\"fn") || rust_type.contains("extern\"C\"") {
            return Self::convert_function_pointer(rust_type);
        }

        if rust_type.starts_with("*const*const")
            || rust_type.starts_with("*mut*const")
            || rust_type.starts_with("*const*mut")
            || rust_type.starts_with("*mut*mut")
        {
            return String::from("bigint");
        }

        if rust_type.starts_with('*') {
            return String::from("bigint");
        }

        if rust_type.contains("c_void") {
            return String::from("void");
        }

        match rust_type {
            "u64" | "i64" => String::from("bigint"),
            "u32" | "i32" | "u16" | "i16" | "u8" | "i8" => String::from("number"),
            "usize" | "isize" => String::from("number"),
            "bool" => String::from("boolean"),
            "()" => String::from("void"),
            other => String::from(other),
        }
    }

    fn convert_function_pointer(type_name: &str) -> String {
        let return_type: &str = if let Some(pos) = type_name.find(")->") {
            &type_name[pos + 3..]
        } else {
            "void"
        };

        let ts_return: String = Self::rust_type_to_ts(return_type);

        let params_start: usize = type_name.find("fn(").map(|p| p + 3).unwrap_or(0);

        let params_end: usize = if let Some(pos) = type_name.find(")->") {
            pos
        } else {
            let mut depth: i32 = 0;
            let mut end_pos: usize = type_name.len();
            for (i, c) in type_name[params_start..].chars().enumerate() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth < 0 {
                            end_pos = params_start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            end_pos
        };

        if params_start == 0 || params_end <= params_start {
            return format!("() => {}", ts_return);
        }

        let params_str: &str = &type_name[params_start..params_end];
        let params: Vec<String> = Self::parse_function_params(params_str);

        if params.is_empty() {
            return format!("() => {}", ts_return);
        }

        format!("({}) => {}", params.join(", "), ts_return)
    }

    fn parse_function_params(params_str: &str) -> Vec<String> {
        if params_str.is_empty() {
            return Vec::new();
        }

        let mut params: Vec<String> = Vec::new();
        let mut current_param: String = String::new();
        let mut depth: i32 = 0;
        let mut param_index: usize = 0;

        for c in params_str.chars() {
            match c {
                '(' | '<' | '[' => {
                    depth += 1;
                    current_param.push(c);
                }
                ')' | '>' | ']' => {
                    depth -= 1;
                    current_param.push(c);
                }
                ',' if depth == 0 => {
                    let param: String = current_param.trim().to_string();
                    if !param.is_empty() {
                        params.push(Self::convert_param(&param, param_index));
                        param_index += 1;
                    }
                    current_param.clear();
                }
                _ => {
                    current_param.push(c);
                }
            }
        }

        if !current_param.trim().is_empty() {
            params.push(Self::convert_param(current_param.trim(), param_index));
        }

        params
    }

    fn convert_param(param: &str, index: usize) -> String {
        let parts: Vec<&str> = param.splitn(2, ':').collect();
        let type_part: &str = if parts.len() == 2 { parts[1] } else { parts[0] };
        let ts_type: String = Self::rust_type_to_ts(type_part.trim());
        let name: &str = if parts.len() == 2 {
            parts[0].trim()
        } else {
            match index {
                0 => "arg0",
                1 => "arg1",
                2 => "arg2",
                3 => "arg3",
                4 => "arg4",
                5 => "arg5",
                _ => {
                    return format!("arg{}: {}", index, ts_type);
                }
            }
        };
        format!("{}: {}", name, ts_type)
    }

    fn format_jsdoc(doc: &str, indent: usize) -> String {
        let indent_str: String = " ".repeat(indent);
        let lines: Vec<&str> = doc.lines().collect();
        if lines.len() == 1 {
            format!("{}/** {} */\n", indent_str, lines[0])
        } else {
            let mut output: String = format!("{}/**\n", indent_str);
            for line in lines {
                output.push_str(&format!("{} * {}\n", indent_str, line));
            }
            output.push_str(&format!("{} */\n", indent_str));
            output
        }
    }
}

impl CodeGenerator for JsGenerator {
    fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
        let ts_type: String = Self::rust_type_to_ts(&item.rust_type);
        let formatted_value: String = if ts_type == "bigint" {
            format!("{}n", item.value)
        } else {
            item.value.clone()
        };

        format!(
            "export const {}: {} = {};\n\n",
            item.name, ts_type, formatted_value
        )
    }

    fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export interface {} {{\n", item.name));

        for field in &item.fields {
            if let Some(doc) = &field.doc {
                output.push_str(&Self::format_jsdoc(doc, 4));
            }

            let ts_type: String = Self::rust_type_to_ts(&field.rust_type);
            output.push_str(&format!("    {}: {};\n", field.name, ts_type));
        }

        output.push_str("}\n\n");
        output
    }

    fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export const enum {} {{\n", item.name));

        for (i, variant) in item.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                output.push_str(&Self::format_jsdoc(doc, 4));
            }

            if let Some(value) = variant.value {
                output.push_str(&format!("    {} = {},\n", variant.name, value));
            } else if i == 0 {
                output.push_str(&format!("    {} = 0,\n", variant.name));
            } else {
                output.push_str(&format!("    {},\n", variant.name));
            }
        }

        output.push_str("}\n\n");
        output
    }

    fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
        let mut output: String = String::new();

        if let Some(doc) = &item.doc {
            output.push_str(&Self::format_jsdoc(doc, 0));
        }

        output.push_str(&format!("export type {} =\n", item.name));

        for variant in item.variants.iter() {
            let ts_type: String = Self::rust_type_to_ts(&variant.type_name);
            output.push_str(&format!("    | {{ {}: {} }}\n", variant.name, ts_type));
        }

        output.push_str(";\n\n");
        output
    }

    fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
        let ret_type: String = item
            .return_type
            .as_ref()
            .map(|t| Self::rust_type_to_ts(t))
            .unwrap_or_else(|| "void".to_string());

        let params: String = item
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, Self::rust_type_to_ts(&p.rust_type)))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "export function {}({}): {} {{}}\n\n",
            item.name, params, ret_type
        )
    }

    fn file_extension(&self) -> &'static str {
        "ts"
    }

    fn language_name(&self) -> &'static str {
        "js"
    }

    fn generate_header(&self, _ctx: &GenerationContext) -> String {
        String::new()
    }
}

impl Default for JsGenerator {
    fn default() -> Self {
        Self::new()
    }
}
