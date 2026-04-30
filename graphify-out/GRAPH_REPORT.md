# Graph Report - crates and sdks  (2026-04-23)

## Corpus Check
- 294 files · ~261,270 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 3040 nodes · 8388 edges · 72 communities detected
- Extraction: 62% EXTRACTED · 37% INFERRED · 0% AMBIGUOUS · INFERRED: 3093 edges (avg confidence: 0.78)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Community 0|Community 0]]
- [[_COMMUNITY_Community 1|Community 1]]
- [[_COMMUNITY_Community 2|Community 2]]
- [[_COMMUNITY_Community 3|Community 3]]
- [[_COMMUNITY_Community 4|Community 4]]
- [[_COMMUNITY_Community 5|Community 5]]
- [[_COMMUNITY_Community 6|Community 6]]
- [[_COMMUNITY_Community 7|Community 7]]
- [[_COMMUNITY_Community 8|Community 8]]
- [[_COMMUNITY_Community 9|Community 9]]
- [[_COMMUNITY_Community 10|Community 10]]
- [[_COMMUNITY_Community 11|Community 11]]
- [[_COMMUNITY_Community 12|Community 12]]
- [[_COMMUNITY_Community 13|Community 13]]
- [[_COMMUNITY_Community 14|Community 14]]
- [[_COMMUNITY_Community 15|Community 15]]
- [[_COMMUNITY_Community 16|Community 16]]
- [[_COMMUNITY_Community 17|Community 17]]
- [[_COMMUNITY_Community 18|Community 18]]
- [[_COMMUNITY_Community 19|Community 19]]
- [[_COMMUNITY_Community 20|Community 20]]
- [[_COMMUNITY_Community 21|Community 21]]
- [[_COMMUNITY_Community 22|Community 22]]
- [[_COMMUNITY_Community 23|Community 23]]
- [[_COMMUNITY_Community 24|Community 24]]
- [[_COMMUNITY_Community 25|Community 25]]
- [[_COMMUNITY_Community 26|Community 26]]
- [[_COMMUNITY_Community 27|Community 27]]
- [[_COMMUNITY_Community 29|Community 29]]
- [[_COMMUNITY_Community 30|Community 30]]
- [[_COMMUNITY_Community 31|Community 31]]
- [[_COMMUNITY_Community 32|Community 32]]
- [[_COMMUNITY_Community 33|Community 33]]
- [[_COMMUNITY_Community 34|Community 34]]
- [[_COMMUNITY_Community 35|Community 35]]
- [[_COMMUNITY_Community 36|Community 36]]
- [[_COMMUNITY_Community 37|Community 37]]
- [[_COMMUNITY_Community 38|Community 38]]
- [[_COMMUNITY_Community 39|Community 39]]
- [[_COMMUNITY_Community 40|Community 40]]
- [[_COMMUNITY_Community 41|Community 41]]
- [[_COMMUNITY_Community 42|Community 42]]
- [[_COMMUNITY_Community 43|Community 43]]
- [[_COMMUNITY_Community 44|Community 44]]
- [[_COMMUNITY_Community 45|Community 45]]
- [[_COMMUNITY_Community 46|Community 46]]
- [[_COMMUNITY_Community 47|Community 47]]
- [[_COMMUNITY_Community 48|Community 48]]
- [[_COMMUNITY_Community 50|Community 50]]
- [[_COMMUNITY_Community 52|Community 52]]
- [[_COMMUNITY_Community 53|Community 53]]
- [[_COMMUNITY_Community 54|Community 54]]
- [[_COMMUNITY_Community 55|Community 55]]
- [[_COMMUNITY_Community 56|Community 56]]
- [[_COMMUNITY_Community 57|Community 57]]
- [[_COMMUNITY_Community 58|Community 58]]
- [[_COMMUNITY_Community 60|Community 60]]
- [[_COMMUNITY_Community 61|Community 61]]
- [[_COMMUNITY_Community 62|Community 62]]
- [[_COMMUNITY_Community 63|Community 63]]
- [[_COMMUNITY_Community 71|Community 71]]
- [[_COMMUNITY_Community 72|Community 72]]
- [[_COMMUNITY_Community 73|Community 73]]
- [[_COMMUNITY_Community 80|Community 80]]
- [[_COMMUNITY_Community 81|Community 81]]
- [[_COMMUNITY_Community 82|Community 82]]
- [[_COMMUNITY_Community 83|Community 83]]
- [[_COMMUNITY_Community 84|Community 84]]
- [[_COMMUNITY_Community 85|Community 85]]
- [[_COMMUNITY_Community 86|Community 86]]
- [[_COMMUNITY_Community 87|Community 87]]
- [[_COMMUNITY_Community 88|Community 88]]

## God Nodes (most connected - your core abstractions)
1. `to_string()` - 194 edges
2. `polyplug_runtime_create()` - 56 edges
3. `polyplug_runtime_destroy()` - 53 edges
4. `Runtime` - 47 edges
5. `split()` - 45 edges
6. `parse_api_str()` - 44 edges
7. `guest_contract_id()` - 44 edges
8. `Builder` - 40 edges
9. `starts_with()` - 35 edges
10. `generate()` - 32 edges

## Surprising Connections (you probably didn't know these)
- `Return the loaded native library instance.      Returns:         ctypes.CDLL: Th` --uses--> `Runtime`  [INFERRED]
  /mnt/data/Projects/Utils/polyplug/sdks/python/host/polyplug/__init__.py → /mnt/data/Projects/Utils/polyplug/sdks/cpp/host/polyplug/runtime.hpp
- `alloc_string()` --calls--> `polyplug_host_alloc()`  [INFERRED]
  /mnt/data/Projects/Utils/polyplug/sdks/cpp/abi/polyplug/abi.hpp → /mnt/data/Projects/Utils/polyplug/crates/polyplug_abi/src/ffi.rs
- `setup_runtime_with_plugin()` --calls--> `guest_contract_id()`  [INFERRED]
  /mnt/data/Projects/Utils/polyplug/crates/polyplug/benches/ffi_resolve.rs → /mnt/data/Projects/Utils/polyplug/crates/polyplug_utils/src/lib.rs
- `setup_runtime_with_plugins()` --calls--> `guest_contract_id()`  [INFERRED]
  /mnt/data/Projects/Utils/polyplug/crates/polyplug/benches/ffi_find_all.rs → /mnt/data/Projects/Utils/polyplug/crates/polyplug_utils/src/lib.rs
- `bench_alloc()` --calls--> `polyplug_host_alloc()`  [INFERRED]
  /mnt/data/Projects/Utils/polyplug/crates/polyplug/benches/contract_dispatch.rs → /mnt/data/Projects/Utils/polyplug/crates/polyplug_abi/src/ffi.rs

## Communities

### Community 0 - "Community 0"
Cohesion: 0.01
Nodes (343): GuestContractHandle, GuestContractId, GuestContractInstance, GuestContractInterface, HostContractInstance, HostContractInterface, HostInterface, PluginDescriptor (+335 more)

### Community 1 - "Community 1"
Cohesion: 0.01
Nodes (343): AbiBuiltin, CodeGenerator, GeneratedFile, PrimitiveType, ResolvedBundle, ResolvedContract, ResolvedDependency, ResolvedHostContract (+335 more)

### Community 2 - "Community 2"
Cohesion: 0.01
Nodes (169): BundleId, AbiError, Range, Config, bench_register_callback(), by_bundle_factory(), by_contract_factory(), DependencyInfo (+161 more)

### Community 3 - "Community 3"
Cohesion: 0.03
Nodes (122): alloc_string(), ends_with(), split(), starts_with(), strip_prefix(), to_str(), to_string(), to_string_view() (+114 more)

### Community 4 - "Community 4"
Cohesion: 0.03
Nodes (129): AbiError, AbiErrorCode, Array, Buffer, BundleInitContext, ContractType, DependencyInfo, DispatchMechanisms (+121 more)

### Community 5 - "Community 5"
Cohesion: 0.05
Nodes (112): BundleLoader, dotnet_config_custom_min_framework(), dotnet_config_default_hostfxr_is_auto(), dotnet_config_default_min_framework_is_net10(), dotnet_loader_runtime_name_is_dotnet(), full_clr_init_reaches_init_symbol_check(), hostfxr_location_default_is_auto(), load_dll_net10_against_net6_requirement_returns_init_failed() (+104 more)

### Community 6 - "Community 6"
Cohesion: 0.03
Nodes (126): bundle_name_conflicts_with_contract_name(), bundle_name_no_conflict_with_contract_names(), check_bundle_name_conflict(), check_enum_chained_refs(), abi_builtins_accepted_as_type_refs(), api_err(), bundle_err(), bundle_missing_name_field() (+118 more)

### Community 7 - "Community 7"
Cohesion: 0.03
Nodes (76): AstGrepError, AstGrepRunner, ByteOffset, capitalize_first(), generate_cpp_pattern(), generate_csharp_pattern(), generate_javascript_pattern(), generate_python_pattern() (+68 more)

### Community 8 - "Community 8"
Cohesion: 0.03
Nodes (89): main(), Args, DotnetConfig, HostfxrLocation, GenerationContext, Language, bench_cached_dispatch(), bench_clr_dispatch() (+81 more)

### Community 9 - "Community 9"
Cohesion: 0.07
Nodes (71): Compatibility, How strictly version compatibility is enforced when resolving plugins., dotnet_loader_new_does_not_panic(), polyplug_python_loader_free(), explicit_load_bundle_missing_manifest_errors(), concurrent_lookups_multiple_contracts(), concurrent_register_and_lookup(), concurrent_register_different_contracts() (+63 more)

### Community 10 - "Community 10"
Cohesion: 0.05
Nodes (77): arg_pack_struct_name(), build_python_sig_params(), collect_python_guest_host_contract_type_imports(), collect_python_type_imports(), collect_type_refs(), contract_name_to_guest_trait(), contract_name_to_struct(), contract_name_to_upper_snake() (+69 more)

### Community 11 - "Community 11"
Cohesion: 0.04
Nodes (55): Version, CapabilityGraph, ContractCapability, noop_call_guest_method(), chain_loads_in_dependency_order(), cycle_detected_with_clear_error(), empty_manifest(), malformed_manifest_skips_bundle() (+47 more)

### Community 12 - "Community 12"
Cohesion: 0.05
Nodes (47): parse_version(), polyplug_python_loader_create(), PolyplugPythonConfig, AbiBuiltin, EnumDef, EnumVariant, primitive_type_roundtrip(), PrimitiveType (+39 more)

### Community 13 - "Community 13"
Cohesion: 0.03
Nodes (24): AllAbiTypesStruct, AllU8Fields, BufferFollowedByU32, EmptyStruct, EnumThenStringView, EnumU16, EnumU32, EnumU64 (+16 more)

### Community 14 - "Community 14"
Cohesion: 0.1
Nodes (29): python_array_field_expands(), python_cfunctype_option_nullable(), python_cfunctype_uses_ctypes_params(), python_fn_ptr_with_const_ptr_param(), python_struct_with_fn_ptr_generates_cfunctype(), PythonGenerator, to_snake_case(), cpp_array_field_generates_void_and_size_t() (+21 more)

### Community 15 - "Community 15"
Cohesion: 0.14
Nodes (23): bridge_call_host_contract_exception(), bridge_call_host_contract_not_callable(), bridge_call_host_contract_not_found(), bridge_call_host_contract_returns_error_code(), bridge_call_host_contract_success(), bridge_call_host_contract_with_fn_id(), bridge_context_returns_reference(), bridge_default_creates_empty_bridge() (+15 more)

### Community 16 - "Community 16"
Cohesion: 0.1
Nodes (21): ends_with(), lua_array_field_expands(), lua_fn_ptr_typedef_uses_c_types(), lua_fn_ptr_void_return_no_extra_parens(), LuaGenerator, cpp_enum_u64_repr_maps_to_uint64_t(), cpp_i64_field_maps_to_int64_t(), cpp_struct_has_no_alignas_specifier() (+13 more)

### Community 17 - "Community 17"
Cohesion: 0.17
Nodes (32): assert_failure_contains(), assert_lang_alias_accepted(), assert_success(), generate_api_and_bundle_conflict_fails(), generate_invalid_lang_empty_string_fails(), generate_invalid_lang_fails(), generate_lang_alias_c_hash_accepted(), generate_lang_alias_cpp_accepted() (+24 more)

### Community 18 - "Community 18"
Cohesion: 0.11
Nodes (33): DotnetLoader, VmLoaderData, GuestContractInstance, GuestContractInterface, HostContractInstance, HostContractInterface, HostInterface, RuntimeInterface (+25 more)

### Community 19 - "Community 19"
Cohesion: 0.18
Nodes (4): csharp_array_field_expands(), csharp_delegate_uses_csharp_types(), csharp_struct_with_fn_ptr_generates_delegate(), CSharpGenerator

### Community 20 - "Community 20"
Cohesion: 0.08
Nodes (8): HostContractId, BundleId, HostContractId, HostContractInterface, contract_id(), contract_id_canonical_format(), fnv1a_64(), GuestError

### Community 21 - "Community 21"
Cohesion: 0.18
Nodes (14): create_test_report(), Reporter, test_default_reporter(), test_find_methods_missing_everywhere(), test_generate_json_empty_report(), test_generate_json_method_status(), test_generate_json_structure(), test_generate_table_header() (+6 more)

### Community 22 - "Community 22"
Cohesion: 0.18
Nodes (4): cpp_array_field_expands(), cpp_fn_ptr_typedef_uses_c_types(), cpp_fn_ptr_void_return_correct(), CppGenerator

### Community 23 - "Community 23"
Cohesion: 0.18
Nodes (5): js_array_field_expands(), js_fn_ptr_field_emits_as_number(), js_struct_emits_interface_and_offsets(), JsGenerator, to_upper_snake_case()

### Community 24 - "Community 24"
Cohesion: 0.3
Nodes (24): pack(), all_python_scaffold_files_carry_generated_header(), all_rust_scaffold_files_carry_generated_header(), assert_contains(), cpp_cmake_contains_bundle_name_and_version(), cpp_header_has_generated_header_comment(), cpp_scaffold_file_structure(), csharp_csproj_contains_version() (+16 more)

### Community 25 - "Community 25"
Cohesion: 0.11
Nodes (11): AbiItems, ConstInfo, EnumInfo, EnumVariant, FieldInfo, FunctionInfo, Item, ParamInfo (+3 more)

### Community 26 - "Community 26"
Cohesion: 0.17
Nodes (21): abi_symbol:loader_create, abi_symbol:register_loader, config:DotnetConfig, config:JsConfig, config:LuaConfig, config:NativeConfig, config:PythonConfig, loader:dotnet:deno (+13 more)

### Community 27 - "Community 27"
Cohesion: 0.24
Nodes (17): all_contract_functions_appear_in_interface(), fns_array_size_equals_declared_function_count(), generate_guest_contracts(), generate_guest_interfaces(), interface_function_count_matches_contract(), interface_slots_are_sequential(), interface_wrapper_function_id_comments_match_slot(), ir_to_api_toml() (+9 more)

### Community 29 - "Community 29"
Cohesion: 0.14
Nodes (13): Compatibility, Tests for Python SDK RuntimeConfig matching polyplug_abi RuntimeConfig., FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (16 bytes)., RuntimeConfig must have compatibility field., RuntimeConfig field types must match polyplug_abi., RuntimeConfig must be 16 bytes to match polyplug_abi., Compatibility enum constants must be defined., Compatibility enum matching polyplug_abi::Compatibility. (+5 more)

### Community 30 - "Community 30"
Cohesion: 0.17
Nodes (6): CTypesBackend, Create runtime with options and return HostInterface pointer., Destroy HostInterface and runtime., Load HostInterface struct from pointer., ctypes-based FFI backend for HostInterface operations., Create runtime and return HostInterface pointer.

### Community 31 - "Community 31"
Cohesion: 0.22
Nodes (7): GenerateConfig, GeneratedFile, GenerateOutput, Lang, PlatformKey, ResolvedBundleFile, Side

### Community 32 - "Community 32"
Cohesion: 0.29
Nodes (3): Protocol, Backend, Protocol for HostInterface-based FFI backend.      Only two FFI bindings needed:

### Community 33 - "Community 33"
Cohesion: 0.33
Nodes (2): null_loader_data(), VmLoaderData

### Community 34 - "Community 34"
Cohesion: 0.38
Nodes (2): GuestContractHandle, test_guest_contract_handle_null()

### Community 35 - "Community 35"
Cohesion: 0.4
Nodes (1): StringView

### Community 36 - "Community 36"
Cohesion: 0.33
Nodes (2): LanguageValidator, ValidationResult

### Community 37 - "Community 37"
Cohesion: 0.33
Nodes (1): RuntimeInterface

### Community 38 - "Community 38"
Cohesion: 0.4
Nodes (1): Buffer

### Community 39 - "Community 39"
Cohesion: 0.5
Nodes (1): AbiErrorCode

### Community 40 - "Community 40"
Cohesion: 0.5
Nodes (2): default_runtime_config(), RuntimeConfig

### Community 41 - "Community 41"
Cohesion: 0.4
Nodes (2): CodeGenerator, GenerationContext

### Community 42 - "Community 42"
Cohesion: 0.4
Nodes (1): VmDispatch

### Community 43 - "Community 43"
Cohesion: 0.5
Nodes (1): BundleInitContext

### Community 44 - "Community 44"
Cohesion: 0.5
Nodes (1): Compatibility

### Community 45 - "Community 45"
Cohesion: 0.5
Nodes (2): LuaConfig, LuaVersion

### Community 46 - "Community 46"
Cohesion: 0.67
Nodes (1): DispatchType

### Community 47 - "Community 47"
Cohesion: 0.67
Nodes (1): NativeDispatch

### Community 48 - "Community 48"
Cohesion: 0.67
Nodes (1): PythonConfig

### Community 50 - "Community 50"
Cohesion: 0.67
Nodes (1): Contract

### Community 52 - "Community 52"
Cohesion: 0.67
Nodes (3): test_layout, polyplug_abi, abi

### Community 53 - "Community 53"
Cohesion: 0.67
Nodes (3): test_layout, polyplug_abi, abi

### Community 54 - "Community 54"
Cohesion: 1.0
Nodes (1): BundleLoader

### Community 55 - "Community 55"
Cohesion: 1.0
Nodes (1): LoadedBundle

### Community 56 - "Community 56"
Cohesion: 1.0
Nodes (1): BundleNode

### Community 57 - "Community 57"
Cohesion: 1.0
Nodes (1): RuntimeLanguage

### Community 58 - "Community 58"
Cohesion: 1.0
Nodes (1): ContractType

### Community 60 - "Community 60"
Cohesion: 1.0
Nodes (1): CodeGenerator

### Community 61 - "Community 61"
Cohesion: 1.0
Nodes (1): JsConfig

### Community 62 - "Community 62"
Cohesion: 1.0
Nodes (1): NativeConfig

### Community 63 - "Community 63"
Cohesion: 1.0
Nodes (2): C++ SDK, C# SDK

### Community 71 - "Community 71"
Cohesion: 1.0
Nodes (1): Register a callback for hot-reload notifications.          Must be called before

### Community 72 - "Community 72"
Cohesion: 1.0
Nodes (1): Set runtime configuration for subsequently created runtimes.          Must be ca

### Community 73 - "Community 73"
Cohesion: 1.0
Nodes (1): Internal: Create a C-compatible callback wrapper.

### Community 80 - "Community 80"
Cohesion: 1.0
Nodes (1): loader:js:lua

### Community 81 - "Community 81"
Cohesion: 1.0
Nodes (1): loader:dotnet:lua

### Community 82 - "Community 82"
Cohesion: 1.0
Nodes (1): loader:python:lua

### Community 83 - "Community 83"
Cohesion: 1.0
Nodes (1): abi_symbol:loader_free

### Community 84 - "Community 84"
Cohesion: 1.0
Nodes (1): LoadedBundle

### Community 85 - "Community 85"
Cohesion: 1.0
Nodes (1): ReloadEvent

### Community 86 - "Community 86"
Cohesion: 1.0
Nodes (1): HostContractInterface

### Community 87 - "Community 87"
Cohesion: 1.0
Nodes (1): PluginDescriptor

### Community 88 - "Community 88"
Cohesion: 1.0
Nodes (1): polyplug_runtime_create

## Knowledge Gaps
- **237 isolated node(s):** `AddArgs`, `FillArgs`, `LoaderError`, `RegistryError`, `GraphError` (+232 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 33`** (7 nodes): `vm_loader_data.rs`, `layout_vm_loader_data()`, `null_loader_data()`, `vm_loader_data_repr_c()`, `VmLoaderData`, `.is_null()`, `.null()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 34`** (7 nodes): `guest_contract_handle.rs`, `GuestContractHandle`, `.is_null()`, `.null()`, `.pack()`, `layout_guest_contract_handle()`, `test_guest_contract_handle_null()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 35`** (6 nodes): `string_view.rs`, `layout_string_view()`, `StringView`, `.as_str()`, `.null()`, `.to_owned_string()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 36`** (6 nodes): `mod.rs`, `LanguageValidator`, `ValidationResult`, `.completion_percentage()`, `.is_complete()`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 37`** (6 nodes): `runtime_interface.rs`, `get_dependencies_field_exists()`, `layout_runtime_interface()`, `list_bundles_field_exists()`, `runtime_interface_has_runtime_field()`, `RuntimeInterface`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 38`** (5 nodes): `Buffer`, `.as_mut_slice()`, `.as_slice()`, `layout_buffer()`, `buffer.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 39`** (5 nodes): `error_code.rs`, `AbiErrorCode`, `.fmt()`, `.from()`, `.from_u32()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 40`** (5 nodes): `runtime_config.rs`, `default_runtime_config()`, `layout_runtime_config()`, `RuntimeConfig`, `.default()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 41`** (5 nodes): `mod.rs`, `CodeGenerator`, `GenerationContext`, `.new()`, `.with_namespace()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 42`** (5 nodes): `vm_dispatch.rs`, `layout_vm_dispatch()`, `vm_dispatch_instance_is_guest_contract_instance()`, `vm_dispatch_uses_vm_loader_data()`, `VmDispatch`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 43`** (4 nodes): `plugin_context.rs`, `bundle_init_context_layout()`, `bundle_init_context_no_bare_c_void()`, `BundleInitContext`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 44`** (4 nodes): `Compatibility`, `compatibility_default_is_strict()`, `compatibility_repr_u32()`, `compatibility.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 45`** (4 nodes): `LuaConfig`, `.default()`, `LuaVersion`, `config.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 46`** (3 nodes): `dispatch_type.rs`, `DispatchType`, `layout_dispatch_type()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 47`** (3 nodes): `native_dispatch.rs`, `layout_native_dispatch()`, `NativeDispatch`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 48`** (3 nodes): `PythonConfig`, `.default()`, `config.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 50`** (3 nodes): `Contract`, `.Contract()`, `contract.hpp`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 54`** (2 nodes): `BundleLoader`, `bundle_loader.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 55`** (2 nodes): `loaded_bundle.rs`, `LoadedBundle`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 56`** (2 nodes): `BundleNode`, `bundle_node.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 57`** (2 nodes): `runtime_language.rs`, `RuntimeLanguage`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 58`** (2 nodes): `ContractType`, `contract_type.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 60`** (2 nodes): `generator.rs`, `CodeGenerator`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 61`** (2 nodes): `JsConfig`, `config.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 62`** (2 nodes): `NativeConfig`, `config.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 63`** (2 nodes): `C++ SDK`, `C# SDK`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 71`** (1 nodes): `Register a callback for hot-reload notifications.          Must be called before`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 72`** (1 nodes): `Set runtime configuration for subsequently created runtimes.          Must be ca`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 73`** (1 nodes): `Internal: Create a C-compatible callback wrapper.`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 80`** (1 nodes): `loader:js:lua`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 81`** (1 nodes): `loader:dotnet:lua`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 82`** (1 nodes): `loader:python:lua`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 83`** (1 nodes): `abi_symbol:loader_free`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 84`** (1 nodes): `LoadedBundle`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 85`** (1 nodes): `ReloadEvent`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 86`** (1 nodes): `HostContractInterface`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 87`** (1 nodes): `PluginDescriptor`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 88`** (1 nodes): `polyplug_runtime_create`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `to_string()` connect `Community 3` to `Community 0`, `Community 1`, `Community 2`, `Community 4`, `Community 5`, `Community 7`, `Community 8`, `Community 9`, `Community 11`, `Community 14`, `Community 15`, `Community 16`, `Community 19`, `Community 21`, `Community 22`, `Community 23`, `Community 24`?**
  _High betweenness centrality (0.130) - this node is a cross-community bridge._
- **Why does `parse_api_str()` connect `Community 6` to `Community 8`, `Community 1`?**
  _High betweenness centrality (0.036) - this node is a cross-community bridge._
- **Why does `Runtime` connect `Community 2` to `Community 9`, `Community 4`, `Community 5`?**
  _High betweenness centrality (0.035) - this node is a cross-community bridge._
- **Are the 192 inferred relationships involving `to_string()` (e.g. with `polyplug_error_undeclared_dependency_display()` and `polyplug_error_dependency_not_found_display()`) actually correct?**
  _`to_string()` has 192 INFERRED edges - model-reasoned connections that need verification._
- **Are the 42 inferred relationships involving `polyplug_runtime_create()` (e.g. with `setup_runtime_with_plugin()` and `setup_runtime_with_plugins()`) actually correct?**
  _`polyplug_runtime_create()` has 42 INFERRED edges - model-reasoned connections that need verification._
- **Are the 38 inferred relationships involving `polyplug_runtime_destroy()` (e.g. with `bench_ffi_resolve_plugin()` and `bench_ffi_resolve_null_handle()`) actually correct?**
  _`polyplug_runtime_destroy()` has 38 INFERRED edges - model-reasoned connections that need verification._
- **Are the 9 inferred relationships involving `Runtime` (e.g. with `polyplug_abi — ABI types for the polyplug plugin runtime.  This package provides` and `Return the loaded native library instance.      Returns:         ctypes.CDLL: Th`) actually correct?**
  _`Runtime` has 9 INFERRED edges - model-reasoned connections that need verification._