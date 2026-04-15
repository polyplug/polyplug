//! Typed function pointer generation tests for all 5 codegen generators.
//!
//! Verifies that each generator produces typed function pointer signatures
//! (not opaque void*) for fields with extern "C" fn types.
//!
//! Test Categories:
//! - Python: CFUNCTYPE typedefs for fn ptr fields
//! - C#: Delegate definitions for fn ptr fields
//! - Lua: Typed fn ptr typedefs in ffi.cdef
//! - JS: Both TypeScript interfaces AND binary offset constants
//! - C++: Typed fn pointer typedefs
//! - Cross-cutting: Array<T>, Option<fn ptr>, tuples

#![allow(clippy::expect_used)]

use polyplug_codegen::data::{FieldInfo, StructInfo};
use polyplug_codegen::languages::{
    CSharpGenerator, CodeGenerator, CppGenerator, GenerationContext, JsGenerator, LuaGenerator,
    PythonGenerator,
};

/// Build a StructInfo for a struct with one function pointer field.
fn make_fn_ptr_struct(name: &str, field_name: &str, fn_ptr_type: &str) -> StructInfo {
    StructInfo {
        name: name.to_string(),
        fields: vec![FieldInfo {
            name: field_name.to_string(),
            rust_type: fn_ptr_type.to_string(),
            doc: None,
        }],
        doc: None,
        attributes: vec!["repr(C)".to_string()],
        size_hint: None,
    }
}

/// Build a StructInfo with multiple fields including fn ptr and regular fields.
fn make_host_interface_struct() -> StructInfo {
    StructInfo {
        name: "HostInterface".to_string(),
        fields: vec![
            FieldInfo {
                name: "runtime".to_string(),
                rust_type: "*mut c_void".to_string(),
                doc: None,
            },
            FieldInfo {
                name: "register_contract".to_string(),
                rust_type: r#"unsafeextern"C"fn(*constHostInterface,*constPluginDescriptor,*constGuestContractInterface)->AbiError"#.to_string(),
                doc: None,
            },
            FieldInfo {
                name: "alloc".to_string(),
                rust_type: r#"unsafeextern"C"fn(*constHostInterface,usize,usize)->*mutu8"#.to_string(),
                doc: None,
            },
            FieldInfo {
                name: "free".to_string(),
                rust_type: r#"unsafeextern"C"fn(*constHostInterface,*mutu8,usize,usize)"#.to_string(),
                doc: None,
            },
        ],
        doc: None,
        attributes: vec!["repr(C)".to_string()],
        size_hint: None,
    }
}

/// Build a StructInfo with Option<fn ptr> field (nullable fn ptr).
fn make_optional_fn_ptr_struct() -> StructInfo {
    StructInfo {
        name: "RuntimeConfig".to_string(),
        fields: vec![
            FieldInfo {
                name: "compatibility".to_string(),
                rust_type: "u32".to_string(),
                doc: None,
            },
            FieldInfo {
                name: "hot_reload_enabled".to_string(),
                rust_type: "bool".to_string(),
                doc: None,
            },
            FieldInfo {
                name: "on_reload".to_string(),
                rust_type: r#"Option<unsafeextern"C"fn(ReloadPhase)>"#.to_string(),
                doc: None,
            },
        ],
        doc: None,
        attributes: vec!["repr(C)".to_string()],
        size_hint: None,
    }
}

/// Build a StructInfo with Array<T> field.
fn make_array_struct() -> StructInfo {
    StructInfo {
        name: "BundleList".to_string(),
        fields: vec![FieldInfo {
            name: "bundles".to_string(),
            rust_type: "Array<BundleId>".to_string(),
            doc: None,
        }],
        doc: None,
        attributes: vec!["repr(C)".to_string()],
        size_hint: None,
    }
}

// ─── Python CFUNCTYPE Tests ────────────────────────────────────────────────────

#[test]
fn python_fn_ptr_field_produces_cfunctype() {
    let generator = PythonGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_fn_ptr_struct(
        "TestStruct",
        "callback",
        r#"unsafeextern"C"fn(*constu8,usize)->u32"#,
    );

    let output = generator.generate_struct(&item, &ctx);

    // Python should produce CFUNCTYPE instead of c_void_p for fn ptr fields.
    assert!(
        output.contains("CFUNCTYPE"),
        "Python generator must produce CFUNCTYPE for fn ptr fields. Got:\n{output}"
    );
}

#[test]
fn python_fn_ptr_typedef_before_struct() {
    let generator = PythonGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_host_interface_struct();

    let output = generator.generate_struct(&item, &ctx);

    // The CFUNCTYPE typedef should appear before the class definition.
    let cfunctype_pos = output.find("CFUNCTYPE").expect("Should contain CFUNCTYPE");
    let class_pos = output
        .find("class HostInterface")
        .expect("Should contain class HostInterface");
    assert!(
        cfunctype_pos < class_pos,
        "CFUNCTYPE typedef must appear before the class definition"
    );
}

#[test]
fn python_multiple_fn_ptrs_all_get_cfunctype() {
    let generator = PythonGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_host_interface_struct();

    let output = generator.generate_struct(&item, &ctx);

    // HostInterface has 3 fn ptr fields; all should have CFUNCTYPE.
    let cfunctype_count = output.matches("CFUNCTYPE").count();
    assert!(
        cfunctype_count >= 3,
        "All 3 fn ptr fields must get CFUNCTYPE typedefs. Found {cfunctype_count}"
    );
}

#[test]
fn python_array_field_generates_void_ptr_and_size_t() {
    let generator = PythonGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_array_struct();

    let output = generator.generate_struct(&item, &ctx);

    // Array<T> should generate as void* items + size_t len + size_t align.
    assert!(
        output.contains("c_void_p") && output.contains("c_size_t"),
        "Array<T> must generate as c_void_p + c_size_t fields. Got:\n{output}"
    );
}

// ─── C# Delegate Tests ─────────────────────────────────────────────────────────

#[test]
fn csharp_fn_ptr_field_produces_delegate() {
    let generator = CSharpGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_fn_ptr_struct(
        "TestStruct",
        "Callback",
        r#"unsafeextern"C"fn(*constu8,usize)->u32"#,
    );

    let output = generator.generate_struct(&item, &ctx);

    assert!(
        output.contains("delegate"),
        "C# generator must produce delegate for fn ptr fields. Got:\n{output}"
    );
}

#[test]
fn csharp_delegate_has_unmanaged_function_pointer_attribute() {
    let generator = CSharpGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_fn_ptr_struct(
        "TestStruct",
        "Callback",
        r#"unsafeextern"C"fn(*constu8,usize)->u32"#,
    );

    let output = generator.generate_struct(&item, &ctx);

    assert!(
        output.contains("UnmanagedFunctionPointer"),
        "C# delegate must have [UnmanagedFunctionPointer] attribute. Got:\n{output}"
    );
}

#[test]
fn csharp_delegate_before_struct() {
    let generator = CSharpGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_host_interface_struct();

    let output = generator.generate_struct(&item, &ctx);

    // Delegate definitions should appear before the struct.
    let delegate_pos = output.find("delegate").expect("Should contain delegate");
    let struct_pos = output
        .find("public struct HostInterface")
        .expect("Should contain struct");
    assert!(
        delegate_pos < struct_pos,
        "Delegate definitions must appear before the struct definition"
    );
}

#[test]
fn csharp_array_field_generates_intptr_and_nuint() {
    let generator = CSharpGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_array_struct();

    let output = generator.generate_struct(&item, &ctx);

    // Array<T> in C# should have IntPtr items + nuint len + nuint align.
    assert!(
        output.contains("IntPtr") && output.contains("nuint"),
        "Array<T> must generate as IntPtr + nuint fields. Got:\n{output}"
    );
}

// ─── Lua Typed FFI Tests ───────────────────────────────────────────────────────

#[test]
fn lua_fn_ptr_field_produces_typed_typedef() {
    let generator = LuaGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_fn_ptr_struct(
        "TestStruct",
        "callback",
        r#"unsafeextern"C"fn(*constu8,usize)->u32"#,
    );

    let output = generator.generate_struct(&item, &ctx);

    // Lua should produce a typed function pointer typedef, not void*.
    assert!(
        output.contains("(*)("),
        "Lua generator must produce typed fn ptr typedef. Got:\n{output}"
    );
}

#[test]
fn lua_array_field_generates_void_and_size_t() {
    let generator = LuaGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_array_struct();

    let output = generator.generate_struct(&item, &ctx);

    // Array<T> in Lua should have void* + size_t + size_t.
    assert!(
        output.contains("void*") && output.contains("size_t"),
        "Array<T> must generate as void* + size_t fields. Got:\n{output}"
    );
}

// ─── JS Offset Constants + Interface Tests ─────────────────────────────────────

#[test]
fn js_fn_ptr_field_produces_number_type() {
    let generator = JsGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_fn_ptr_struct(
        "TestStruct",
        "callback",
        r#"unsafeextern"C"fn(*constu8,usize)->u32"#,
    );

    let output = generator.generate_struct(&item, &ctx);

    // JS should produce typed interfaces with fn ptr fields as number.
    assert!(
        output.contains("number") || output.contains("bigint"),
        "JS generator must produce typed interface for fn ptr fields. Got:\n{output}"
    );
}

#[test]
fn js_struct_produces_interface() {
    let generator = JsGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_host_interface_struct();

    let output = generator.generate_struct(&item, &ctx);

    assert!(
        output.contains("export interface"),
        "JS generator must produce export interface. Got:\n{output}"
    );
}

#[test]
fn js_array_field_generates_generic_struct() {
    let generator = JsGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_array_struct();

    let output = generator.generate_struct(&item, &ctx);

    // Array<T> should have the field name and offset constants for len/align.
    assert!(
        output.contains("bundles"),
        "Array<T> must have field name in interface. Got:\n{output}"
    );
    assert!(
        output.contains("_LEN_OFFSET"),
        "Array<T> must have _LEN_OFFSET constant. Got:\n{output}"
    );
    assert!(
        output.contains("_ALIGN_OFFSET"),
        "Array<T> must have _ALIGN_OFFSET constant. Got:\n{output}"
    );
}

// ─── C++ Typed Fn Ptr Tests ────────────────────────────────────────────────────

#[test]
fn cpp_fn_ptr_field_produces_typed_ptr() {
    let generator = CppGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_fn_ptr_struct(
        "TestStruct",
        "callback",
        r#"unsafeextern"C"fn(*constu8,usize)->uint32_t"#,
    );

    let output = generator.generate_struct(&item, &ctx);

    // C++ should produce typed fn pointer in struct.
    assert!(
        output.contains("(*)"),
        "C++ generator must produce typed fn ptr. Got:\n{output}"
    );
}

#[test]
fn cpp_array_field_generates_void_and_size_t() {
    let generator = CppGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_array_struct();

    let output = generator.generate_struct(&item, &ctx);

    assert!(
        output.contains("void*") && output.contains("size_t"),
        "Array<T> must generate as void* + size_t fields. Got:\n{output}"
    );
}

// ─── Option<fn ptr> (nullable fn ptr) Tests ───────────────────────────────────

#[test]
fn python_optional_fn_ptr_generates_cfunctype() {
    let generator = PythonGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_optional_fn_ptr_struct();

    let output = generator.generate_struct(&item, &ctx);

    // Option<fn ptr> should still produce CFUNCTYPE (nullable via ctypes).
    assert!(
        output.contains("CFUNCTYPE"),
        "Option<fn ptr> must produce CFUNCTYPE. Got:\n{output}"
    );
}

#[test]
fn csharp_optional_fn_ptr_generates_delegate() {
    let generator = CSharpGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_optional_fn_ptr_struct();

    let output = generator.generate_struct(&item, &ctx);

    assert!(
        output.contains("delegate"),
        "Option<fn ptr> must produce delegate. Got:\n{output}"
    );
}

// ─── Cross-cutting: No void* for fn ptr fields ─────────────────────────────────

#[test]
fn python_no_void_p_for_fn_ptr_fields() {
    let generator = PythonGenerator::new();
    let ctx = GenerationContext::new();
    let item = make_host_interface_struct();

    let output = generator.generate_struct(&item, &ctx);

    // After the fix, fn ptr fields should reference CFUNCTYPE typedef names in _fields_.
    // The `runtime` field is a raw pointer (*mut c_void) and correctly uses c_void_p.
    // Only fn ptr fields (register_contract, alloc, free) must use CFUNCTYPE names.
    let fields_section = output
        .split("_fields_")
        .nth(1)
        .expect("Should have _fields_ section");

    assert!(
        fields_section.contains("_host_interface_register_contract_t"),
        "register_contract field must use CFUNCTYPE typedef. Got:\n{output}"
    );
    assert!(
        fields_section.contains("_host_interface_alloc_t"),
        "alloc field must use CFUNCTYPE typedef. Got:\n{output}"
    );
    assert!(
        fields_section.contains("_host_interface_free_t"),
        "free field must use CFUNCTYPE typedef. Got:\n{output}"
    );
}
