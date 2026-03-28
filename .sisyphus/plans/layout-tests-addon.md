---

- [ ] 2.5. Add 25+ layout calculation tests in polyplug_codegen

  **CRITICAL**: Comprehensive test suite to verify ALL type layout calculations are accurate!
  
  **Location**: `crates/polyplug_codegen/tests/layout_calculations.rs`
  
  **Purpose**: Ensure polyplugc correctly calculates sizes, alignments, and offsets for ALL types
  
  ---
  
  **Category 1: Primitive Types (6 tests)**
  
  Test file: `tests/layout_primitives.rs`
  
  ```rust
  #[test]
  fn layout_u8_size_align() {
      let ty = PrimitiveType::U8;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 1);
      assert_eq!(layout.align, 1);
  }
  
  #[test]
  fn layout_u16_size_align() {
      let ty = PrimitiveType::U16;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 2);
      assert_eq!(layout.align, 2);
  }
  
  #[test]
  fn layout_u32_size_align() {
      let ty = PrimitiveType::U32;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 4);
      assert_eq!(layout.align, 4);
  }
  
  #[test]
  fn layout_u64_size_align() {
      let ty = PrimitiveType::U64;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 8);
      assert_eq!(layout.align, 8);
  }
  
  #[test]
  fn layout_usize_size_align() {
      let ty = PrimitiveType::USize;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 8);  // x86_64
      assert_eq!(layout.align, 8);
  }
  
  #[test]
  fn layout_bool_size_align() {
      let ty = PrimitiveType::Bool;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 1);
      assert_eq!(layout.align, 1);
  }
  ```
  
  ---
  
  **Category 2: ABI Built-in Types (5 tests)**
  
  Test file: `tests/layout_abi_builtins.rs`
  
  ```rust
  #[test]
  fn layout_stringview_fields_and_size() {
      let ty = AbiBuiltin::StringView;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 16);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields.len(), 2);
      assert_eq!(layout.fields[0].name, "ptr");
      assert_eq!(layout.fields[0].offset, 0);
      assert_eq!(layout.fields[0].size, 8);
      assert_eq!(layout.fields[1].name, "len");
      assert_eq!(layout.fields[1].offset, 8);
      assert_eq!(layout.fields[1].size, 8);
  }
  
  #[test]
  fn layout_buffer_fields_and_size() {
      let ty = AbiBuiltin::Buffer;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 24);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields.len(), 3);
      assert_eq!(layout.fields[0].offset, 0);   // ptr
      assert_eq!(layout.fields[1].offset, 8);   // len
      assert_eq!(layout.fields[2].offset, 16);  // cap
  }
  
  #[test]
  fn layout_abierror_fields_and_size() {
      let ty = AbiBuiltin::AbiError;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 24);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields[0].offset, 0);   // code
      assert_eq!(layout.fields[1].offset, 8);   // message
  }
  
  #[test]
  fn layout_plug Handle_fields_and_size() {
      let ty = AbiBuiltin::PluginHandle;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 8);
      assert_eq!(layout.align, 4);
      assert_eq!(layout.fields[0].offset, 0);   // index
      assert_eq!(layout.fields[1].offset, 4);   // generation
  }
  
  #[test]
  fn layout_hostcontext_fields_and_size() {
      let ty = AbiBuiltin::HostContext;
      let layout = calculate_layout(ty);
      assert_eq!(layout.size, 16);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields[0].offset, 0);   // runtime
      assert_eq!(layout.fields[1].offset, 8);   // bundle_id
  }
  ```
  
  ---
  
  **Category 3: Enum Types (4 tests)**
  
  Test file: `tests/layout_enums.rs`
  
  ```rust
  #[test]
  fn layout_enum_u8_repr() {
      let enum_def = EnumDef {
          name: "TestEnum".to_string(),
          repr: EnumRepr::U8,
          variants: vec![...],
      };
      let layout = calculate_layout(Type::Enum(enum_def));
      assert_eq!(layout.size, 1);
      assert_eq!(layout.align, 1);
  }
  
  #[test]
  fn layout_enum_u16_repr() {
      let enum_def = EnumDef { repr: EnumRepr::U16, ... };
      let layout = calculate_layout(Type::Enum(enum_def));
      assert_eq!(layout.size, 2);
      assert_eq!(layout.align, 2);
  }
  
  #[test]
  fn layout_enum_u32_repr() {
      let enum_def = EnumDef { repr: EnumRepr::U32, ... };
      let layout = calculate_layout(Type::Enum(enum_def));
      assert_eq!(layout.size, 4);
      assert_eq!(layout.align, 4);
  }
  
  #[test]
  fn layout_enum_u64_repr() {
      let enum_def = EnumDef { repr: EnumRepr::U64, ... };
      let layout = calculate_layout(Type::Enum(enum_def));
      assert_eq!(layout.size, 8);
      assert_eq!(layout.align, 8);
  }
  ```
  
  ---
  
  **Category 4: Custom Structs with Padding (6 tests)**
  
  Test file: `tests/layout_structs_with_padding.rs`
  
  ```rust
  #[test]
  fn layout_struct_single_field() {
      let struct_def = StructDef {
          name: "Single".to_string(),
          fields: vec![
              Field { name: "value".to_string(), ty: Type::Primitive(PrimitiveType::U32) },
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 4);
      assert_eq!(layout.align, 4);
      assert_eq!(layout.fields[0].offset, 0);
  }
  
  #[test]
  fn layout_struct_two_fields_no_padding() {
      // u32 (4, align 4) + u32 (4, align 4) = 8, no padding
      let struct_def = StructDef {
          name: "TwoU32".to_string(),
          fields: vec![
              Field { name: "a".to_string(), ty: Type::U32 },
              Field { name: "b".to_string(), ty: Type::U32 },
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 8);
      assert_eq!(layout.align, 4);
      assert_eq!(layout.fields[0].offset, 0);
      assert_eq!(layout.fields[1].offset, 4);
      assert_eq!(layout.padding, vec![]);  // No padding
  }
  
  #[test]
  fn layout_struct_with_padding_u8_u64() {
      // u8 (1, align 1) + padding(7) + u64 (8, align 8) = 16
      let struct_def = StructDef {
          name: "U8ThenU64".to_string(),
          fields: vec![
              Field { name: "small".to_string(), ty: Type::U8 },
              Field { name: "big".to_string(), ty: Type::U64 },
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 16);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields[0].offset, 0);
      assert_eq!(layout.fields[1].offset, 8);
      assert_eq!(layout.padding, vec![Padding { offset: 1, size: 7 }]);
  }
  
  #[test]
  fn layout_logwithlevelargs_accurate() {
      // LogLevel (u32, size 4, align 4) + padding(4) + StringView (16, align 8)
      let struct_def = StructDef {
          name: "LogWithLevelArgs".to_string(),
          fields: vec![
              Field { name: "level".to_string(), ty: Type::Enum(LogLevel) },
              Field { name: "message".to_string(), ty: Type::StringView },
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 24);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields[0].offset, 0);   // level
      assert_eq!(layout.fields[1].offset, 8);   // message (after 4-byte padding)
      assert_eq!(layout.padding, vec![Padding { offset: 4, size: 4 }]);
  }
  
  #[test]
  fn layout_struct_multiple_pad_fields() {
      // u8 + padding(7) + u64 + u16 + padding(6) + StringView
      let struct_def = StructDef {
          name: "Mixed".to_string(),
          fields: vec![
              Field { name: "a".to_string(), ty: Type::U8 },
              Field { name: "b".to_string(), ty: Type::U64 },
              Field { name: "c".to_string(), ty: Type::U16 },
              Field { name: "d".to_string(), ty: Type::StringView },
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 40);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields[0].offset, 0);   // a
      assert_eq!(layout.fields[1].offset, 8);   // b (after 7-byte padding)
      assert_eq!(layout.fields[2].offset, 16);  // c
      assert_eq!(layout.fields[3].offset, 24);  // d (after 6-byte padding)
  }
  
  #[test]
  fn layout_struct_nested() {
      // Nested struct: Outer { inner: Inner, value: u32 }
      // Inner { a: u8, b: u64 } = 16 bytes, align 8
      // Outer = Inner(16) + u32(4) + padding(4) = 24, align 8
      let inner = StructDef { ... };
      let outer = StructDef {
          name: "Outer".to_string(),
          fields: vec![
              Field { name: "inner".to_string(), ty: Type::Struct(inner) },
              Field { name: "value".to_string(), ty: Type::U32 },
          ],
      };
      let layout = calculate_layout(Type::Struct(outer));
      assert_eq!(layout.size, 24);
      assert_eq!(layout.align, 8);
      assert_eq!(layout.fields[0].offset, 0);   // inner (16 bytes)
      assert_eq!(layout.fields[1].offset, 16);  // value
      // 4 bytes padding at end to align to 8
  }
  ```
  
  ---
  
  **Category 5: Complex Multi-Type Structs (4 tests)**
  
  Test file: `tests/layout_complex.rs`
  
  ```rust
  #[test]
  fn layout_complex_function_args() {
      // Simulates: process(data: Buffer, count: u32, flag: bool)
      let struct_def = StructDef {
          name: "ProcessArgs".to_string(),
          fields: vec![
              Field { name: "data".to_string(), ty: Type::Buffer },      // 24 bytes, align 8
              Field { name: "count".to_string(), ty: Type::U32 },       // 4 bytes, align 4
              Field { name: "flag".to_string(), ty: Type::Bool },       // 1 byte, align 1
              // padding to align struct to 8: 3 bytes
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 32);  // 24 + 4 + 1 + 3 padding
      assert_eq!(layout.align, 8);
  }
  
  #[test]
  fn layout_struct_with_enum_and_string() {
      // Mixed: enum (4) + padding(4) + StringView(16) = 24
      let struct_def = StructDef {
          name: "LogEntry".to_string(),
          fields: vec![
              Field { name: "level".to_string(), ty: Type::Enum(LogLevel) },
              Field { name: "message".to_string(), ty: Type::StringView },
          ],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 24);
      assert_eq!(layout.align, 8);
  }
  
  #[test]
  fn layout_empty_struct() {
      let struct_def = StructDef {
          name: "Empty".to_string(),
          fields: vec![],
      };
      let layout = calculate_layout(Type::Struct(struct_def));
      assert_eq!(layout.size, 0);
      assert_eq!(layout.align, 1);
  }
  
  #[test]
  fn layout_union_if_supported() {
      // If polyplug supports unions
      // Union size = max(field sizes), align = max(field aligns)
  }
  ```
  
  ---
  
  **Test Infrastructure**
  
  **Helper Functions in `tests/common/layout.rs`**:
  
  ```rust
  pub struct LayoutInfo {
      pub size: usize,
      pub align: usize,
      pub fields: Vec<FieldLayout>,
      pub padding: Vec<Padding>,
  }
  
  pub struct FieldLayout {
      pub name: String,
      pub offset: usize,
      pub size: usize,
  }
  
  pub struct Padding {
      pub offset: usize,
      pub size: usize,
  }
  
  pub fn calculate_layout(ty: Type) -> LayoutInfo {
      // Uses polyplugc's layout calculation engine
      polyplug_codegen::layout::calculate(ty)
  }
  
  pub fn assert_layout_matches_rust(ty: Type) {
      // Compares polyplugc's calculation with Rust's actual layout
      let calculated = calculate_layout(ty);
      let actual_size = std::mem::size_of_val(&ty);
      let actual_align = std::mem::align_of_val(&ty);
      assert_eq!(calculated.size, actual_size, "Size mismatch");
      assert_eq!(calculated.align, actual_align, "Align mismatch");
  }
  ```
  
  ---
  
  **Running Tests**:
  
  ```bash
  # Run all layout tests
  cargo test --test layout_calculations
  
  # Run specific category
  cargo test --test layout_primitives
  cargo test --test layout_structs_with_padding
  
  # Run with verbose output
  cargo test --test layout_calculations -- --nocapture
  ```
  
  ---
  
  **Acceptance Criteria**:
  - [ ] 25+ layout tests implemented
  - [ ] All primitive types tested
  - [ ] All ABI built-ins tested
  - [ ] All enum repr sizes tested
  - [ ] Struct padding calculation verified
  - [ ] Complex multi-field structs tested
  - [ ] Nested structs tested
  - [ ] All tests pass: `cargo test --test layout_calculations`
  - [ ] Tests verify against Rust's actual layouts
  
  **Commit**: YES
  - Message: `test(polyplug_codegen): add 25+ layout calculation tests`

