//! SDK generation module — integrates language generators from polyplug_codegen.
//!
//! This module provides functions to generate SDK bindings for all supported
//! languages (C++, C#, Python, Lua, JavaScript) from extracted ABI types.
//! After code generation, it preserves hand-written helper method bodies from
//! existing helper files by merging them into the generated output.

#![allow(clippy::std_instead_of_core)]

use crate::mapper::map_all_abi_types;
use crate::types::AbiTypes;
use polyplug_codegen::data::Item;
use polyplug_codegen::languages::{
    CSharpGenerator, CodeGenerator, CppGenerator, ForwardKind, GenerationContext, JsGenerator,
    LuaGenerator, PythonGenerator,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Target language for SDK generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLang {
    /// C++ (C++17 headers).
    Cpp,
    /// C# (.NET bindings).
    CSharp,
    /// Python (ctypes bindings).
    Python,
    /// Lua (LuaJIT FFI bindings).
    Lua,
    /// JavaScript/TypeScript.
    JavaScript,
}

impl TargetLang {
    /// Return the language name for directory structure.
    pub const fn language_name(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "cpp",
            TargetLang::CSharp => "csharp",
            TargetLang::Python => "python",
            TargetLang::Lua => "lua",
            TargetLang::JavaScript => "js",
        }
    }

    /// Return the output filename for the generated SDK.
    pub const fn output_filename(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "abi.hpp",
            TargetLang::CSharp => "Abi.cs",
            TargetLang::Python => "abi.py",
            TargetLang::Lua => "abi.lua",
            TargetLang::JavaScript => "abi.ts",
        }
    }

    /// Return the subdirectory path for the generated SDK.
    pub const fn subdir(&self) -> &'static str {
        match self {
            TargetLang::Cpp => "polyplug",
            TargetLang::CSharp => "",
            TargetLang::Python => "",
            TargetLang::Lua => "",
            TargetLang::JavaScript => "",
        }
    }
}

/// Patterns in rust_type strings that indicate types which cannot be represented
/// in target languages. Simple generics (Array<T>, Option<...>) and tuples are allowed.
const UNREPRESENTABLE_PATTERNS: &[&str] = &["dyn ", "impl ", "for<", "where "];

// ─── Inline Helper Method Source (per language) ───────────────────────────────
//
// Per D-12: Helper methods are embedded as const strings so they survive across
// rebuilds without relying on external files that get deleted after merge.

/// C# StringViewHelper class body (no namespace wrapper, no using statements).
/// Merged into Abi.cs inside the Polyplug.Abi namespace.
const HELPER_CSHARP_STRING_VIEW: &str = r#"
/// <summary>
/// Helpers for constructing and converting StringViews at the ABI boundary.
/// This is the unified implementation used by both host and guest.
/// </summary>
public static class StringViewHelper
{
    /// <summary>
    /// Returns a StringView pointing at the pinned byte array via a GCHandle.
    /// Caller owns the GCHandle and must keep it alive while the StringView is in use.
    /// </summary>
    public static StringView FromPinnedHandle(GCHandle handle, int length) =>
        new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (nuint)length };

    /// <summary>
    /// Returns a StringView pointing at a pre-pinned IntPtr. Caller ensures ptr validity.
    /// </summary>
    public static StringView FromPtr(IntPtr ptr, int length) =>
        new StringView { Ptr = ptr, Len = (nuint)length };

    /// <summary>
    /// Creates a StringView from a .NET string by pinning it in memory.
    /// The GCHandle must be kept alive while the StringView is in use.
    /// For guest plugins, return strings should use host allocation via registrar.
    /// </summary>
    public static (StringView View, GCHandle Handle) FromStringPinned(string str)
    {
        if (string.IsNullOrEmpty(str))
            return (new StringView { Ptr = IntPtr.Zero, Len = 0 }, default);

        byte[] bytes = Encoding.UTF8.GetBytes(str);
        GCHandle handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        StringView sv = new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (nuint)bytes.Length };
        return (sv, handle);
    }

    /// <summary>
    /// Converts a StringView to a .NET string by copying the UTF-8 bytes.
    /// </summary>
    public static string ToString(this StringView sv)
    {
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return string.Empty;

        byte[] bytes = new byte[(int)sv.Len];
        Marshal.Copy(sv.Ptr, bytes, 0, (int)sv.Len);
        return Encoding.UTF8.GetString(bytes);
    }

    /// <summary>
    /// Converts a StringView to a .NET string. Alias for ToString.
    /// </summary>
    public static string ToStr(StringView sv) => ToString(sv);

    /// <summary>
    /// Checks if a StringView starts with the given prefix.
    /// </summary>
    public static bool StartsWith(StringView sv, string prefix)
    {
        if (string.IsNullOrEmpty(prefix))
            return true;
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return false;

        string str = ToString(sv);
        return str.StartsWith(prefix);
    }

    /// <summary>
    /// Checks if a StringView ends with the given suffix.
    /// </summary>
    public static bool EndsWith(StringView sv, string suffix)
    {
        if (string.IsNullOrEmpty(suffix))
            return true;
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return false;

        string str = ToString(sv);
        return str.EndsWith(suffix);
    }

    /// <summary>
    /// Strips the prefix from a StringView if it starts with it.
    /// Returns the original string if the prefix is not present.
    /// </summary>
    public static string StripPrefix(StringView sv, string prefix)
    {
        if (string.IsNullOrEmpty(prefix))
            return ToString(sv);

        string str = ToString(sv);
        if (str.StartsWith(prefix))
            return str.Substring(prefix.Length);
        return str;
    }

    /// <summary>
    /// Splits a StringView by the given delimiter and returns an array of strings.
    /// </summary>
    public static string[] Split(StringView sv, string delimiter)
    {
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return System.Array.Empty<string>();

        string str = ToString(sv);
        if (string.IsNullOrEmpty(delimiter))
            return new[] { str };

        return str.Split(new[] { delimiter }, System.StringSplitOptions.None);
    }
}
"#;

/// C# StringHelpers class body (no namespace wrapper, no using statements).
/// Requires `using System.Runtime.InteropServices;` and `using System.Text;`
/// which are included in the generated Abi.cs header.
const HELPER_CSHARP_STRING_HELPERS: &str = r#"
/// <summary>
/// String helper methods for working with StringView.
/// Native C# implementation for zero overhead.
/// </summary>
public static unsafe class StringHelpers
{
    /// <summary>
    /// Converts a StringView to a .NET string by copying the UTF-8 bytes.
    /// </summary>
    public static string ToString(StringView sv)
    {
        if (sv.Ptr == IntPtr.Zero || sv.Len == 0)
            return string.Empty;

        return Encoding.UTF8.GetString((byte*)sv.Ptr, (int)sv.Len);
    }

    /// <summary>
    /// Strips the prefix from a StringView if it starts with it.
    /// Returns the original string if the prefix is not present.
    /// </summary>
    public static string StripPrefix(StringView sv, string prefix)
    {
        string str = ToString(sv);
        if (str.StartsWith(prefix))
        {
            return str.Substring(prefix.Length);
        }
        return str;
    }

    /// <summary>
    /// Checks if a StringView starts with the given prefix.
    /// </summary>
    public static bool StartsWith(StringView sv, string prefix)
    {
        return ToString(sv).StartsWith(prefix);
    }

    /// <summary>
    /// Splits a StringView by the given delimiter.
    /// </summary>
    public static string[] Split(StringView sv, char delimiter)
    {
        return ToString(sv).Split(delimiter);
    }

    /// <summary>
    /// Returns a process-lifetime <see cref="StringView"/> for a string that
    /// crosses the ABI boundary as an <see cref="AbiError.Message"/>.
    /// </summary>
    /// <remarks>
    /// Per the ABI ownership contract, an AbiError.Message is a static or
    /// runtime-owned string that the receiver MUST NEVER free. This helper pins
    /// the UTF-8 bytes for the lifetime of the process (equivalent to .rodata),
    /// caching one buffer per distinct string so repeated errors never leak a new
    /// GCHandle. It is the only sound way to hand the host a borrowed message that
    /// nobody frees. Use it for fixed error literals — NOT for per-call argument
    /// strings (use <see cref="PinnedUtf8"/> for those).
    /// </remarks>
    public static StringView StaticMessage(string value)
    {
        if (string.IsNullOrEmpty(value))
            return new StringView { Ptr = IntPtr.Zero, Len = 0 };

        lock (s_staticMessages)
        {
            if (s_staticMessages.TryGetValue(value, out StringView cached))
                return cached;

            byte[] bytes = Encoding.UTF8.GetBytes(value);
            // Never freed: pinned for the process lifetime, mirroring .rodata.
            GCHandle handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
            StringView view = new StringView
            {
                Ptr = handle.AddrOfPinnedObject(),
                Len = (nuint)bytes.Length,
            };
            s_staticMessages[value] = view;
            return view;
        }
    }

    private static readonly System.Collections.Generic.Dictionary<string, StringView> s_staticMessages =
        new System.Collections.Generic.Dictionary<string, StringView>();
}

/// <summary>
/// Call-scoped pin of a .NET string's UTF-8 bytes, exposing a borrowed
/// <see cref="StringView"/> for passing a string ARGUMENT across the ABI
/// boundary. The argument is borrowed for the duration of the call only:
/// construct a <see cref="PinnedUtf8"/>, pass <see cref="View"/> into the call,
/// then dispose to release the pin.
/// </summary>
/// <remarks>
/// This is the correct mechanism for argument strings. It pins managed bytes
/// only for the call and frees the <see cref="GCHandle"/> on <see cref="Dispose"/>,
/// so there is no per-call leak. The host never frees this memory — it only reads
/// it for the duration of the call. For AbiError.Message values, use
/// <see cref="StringHelpers.StaticMessage"/> instead.
/// </remarks>
public sealed class PinnedUtf8 : IDisposable
{
    private GCHandle _handle;
    private bool _pinned;

    /// <summary>
    /// The borrowed <see cref="StringView"/> over the pinned UTF-8 bytes. Valid
    /// only until <see cref="Dispose"/> is called.
    /// </summary>
    public StringView View { get; }

    public PinnedUtf8(string value)
    {
        if (string.IsNullOrEmpty(value))
        {
            View = new StringView { Ptr = IntPtr.Zero, Len = 0 };
            return;
        }

        byte[] bytes = Encoding.UTF8.GetBytes(value);
        _handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        _pinned = true;
        View = new StringView
        {
            Ptr = _handle.AddrOfPinnedObject(),
            Len = (nuint)bytes.Length,
        };
    }

    public void Dispose()
    {
        if (_pinned)
        {
            _handle.Free();
            _pinned = false;
        }
    }
}
"#;

/// Lua contract-ID / hash helper function definitions (function M.* only).
///
/// Provenance: mirrors the canonical FNV-1a 64-bit scheme implemented in
/// `crates/polyplug_utils/src/{lib,guest_contract_id,host_contract_id,bundle_id}.rs`.
/// Each function is self-contained (no shared module-level locals) because the
/// Lua helper extractor only preserves `function M.*` blocks.
const HELPER_LUA_HASHING: &str = r#"
--- Compute FNV-1a 64-bit hash of a Lua string.
-- @param str string Input string.
-- @return cdata uint64_t FNV-1a 64-bit hash.
function M.fnv1a_64(str)
    local bit = require("bit")
    local h = 0xcbf29ce484222325ULL
    for i = 1, #str do
        h = bit.bxor(h, str:byte(i))
        h = h * 0x00000100000001B3ULL
    end
    return h
end

--- Compute a bundle ID from its name using FNV-1a 64-bit hash.
-- @param name string     Bundle name.
-- @return cdata uint64_t Bundle ID hash.
function M.bundle_id(name)
    return M.fnv1a_64(name)
end

--- Calculate guest contract ID from name and major version.
-- @param name string          Contract name.
-- @param major_version number Major version.
-- @return cdata uint64_t      Guest contract ID hash.
function M.guest_contract_id(name, major_version)
    return M.fnv1a_64("guest_contract:" .. name .. "@" .. tostring(major_version))
end

--- Calculate host contract ID from name and major version.
-- @param name string          Contract name.
-- @param major_version number Major version.
-- @return cdata uint64_t      Host contract ID hash.
function M.host_contract_id(name, major_version)
    return M.fnv1a_64("host_contract:" .. name .. "@" .. tostring(major_version))
end
"#;

/// Python contract-ID / hash helper definitions appended to abi.py.
///
/// Provenance: mirrors the canonical FNV-1a 64-bit scheme implemented in
/// `crates/polyplug_utils/src/{lib,guest_contract_id,host_contract_id,bundle_id}.rs`.
const HELPER_PYTHON_HASHING: &str = r#"
# ─── FNV-1a 64-bit Hash / Contract-ID Helpers ─────────────────────────────────

FNV_OFFSET: int = 0xCBF29CE484222325
FNV_PRIME: int = 0x00000100000001B3


def fnv1a_64(data: bytes) -> int:
    """Compute FNV-1a 64-bit hash of a byte sequence."""
    hash_val: int = FNV_OFFSET
    for byte in data:
        hash_val ^= byte
        hash_val = (hash_val * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return hash_val


def guest_contract_id(name: str, major_version: int) -> int:
    """Calculate guest contract ID from name and major version."""
    return fnv1a_64(f"guest_contract:{name}@{major_version}".encode("utf-8"))


def host_contract_id(name: str, major_version: int) -> int:
    """Calculate host contract ID from name and major version."""
    return fnv1a_64(f"host_contract:{name}@{major_version}".encode("utf-8"))


def bundle_id(name: str) -> int:
    """Compute a bundle ID from its name using FNV-1a 64-bit hash."""
    return fnv1a_64(name.encode("utf-8"))
"#;

/// Lua helper function definitions (function M.* only, no module boilerplate).
/// Merged into abi.lua before `return M`.
const HELPER_LUA: &str = r#"
--- Convert StringView to Lua string.
-- @param sv StringView from polyplug ABI (ffi.cdata), or nil for a null view
-- @return string Lua string (UTF-8), empty string if nil/empty
-- Raises if given anything other than a StringView cdata or nil — most often a
-- Lua string that was already converted (double-conversion), which would
-- otherwise silently yield "" because a Lua string has no `.ptr` field.
function M.to_str(sv)
    if sv == nil then
        return ""
    end
    if type(sv) ~= "cdata" then
        error("polyplug.to_str: expected a StringView cdata (or nil), got a " ..
            type(sv) .. " — did you already convert it to a Lua string? " ..
            "Pass the original StringView, not its to_str() result.", 2)
    end
    if sv.ptr == nil or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

--- Check if StringView starts with prefix.
-- @param sv StringView from polyplug ABI
-- @param prefix string Prefix string to check for
-- @return boolean True if the string starts with the prefix
function M.starts_with(sv, prefix)
    local s = M.to_str(sv)
    return s:sub(1, #prefix) == prefix
end

--- Check if StringView ends with suffix.
-- @param sv StringView from polyplug ABI
-- @param suffix string Suffix string to check for
-- @return boolean True if the string ends with the suffix
function M.ends_with(sv, suffix)
    local s = M.to_str(sv)
    if #suffix > #s then
        return false
    end
    return s:sub(-#suffix) == suffix
end

--- Strip prefix from StringView if present.
-- @param sv StringView from polyplug ABI
-- @param prefix string Prefix string to strip
-- @return string String with prefix removed if present, otherwise original
function M.strip_prefix(sv, prefix)
    local s = M.to_str(sv)
    if s:sub(1, #prefix) == prefix then
        return s:sub(#prefix + 1)
    end
    return s
end

--- Split StringView by delimiter.
-- @param sv StringView from polyplug ABI
-- @param delimiter string Delimiter string to split by (default: whitespace)
-- @return table Array of strings resulting from the split
function M.split(sv, delimiter)
    local s = M.to_str(sv)
    if s == "" then
        return {}
    end

    delimiter = delimiter or "%s+"
    local result = {}
    local pattern = "(.-)" .. delimiter .. "()"
    local last_pos = 1

    for part, pos in s:gmatch(pattern) do
        table.insert(result, part)
        last_pos = pos
    end

    -- Add the remaining part after the last delimiter
    table.insert(result, s:sub(last_pos))

    return result
end
"#;

/// JavaScript/TypeScript helper function definitions (no import lines).
/// Merged into abi.ts after generated type definitions.
const HELPER_JS: &str = r#"
/**
 * Convert a StringView to a JavaScript string.
 * @param sv - The StringView to convert.
 * @returns The JavaScript string, or empty string if null/empty.
 */
export function stringViewToString(sv: StringView | null | undefined): string {
    if (!sv || sv.ptr === 0n || sv.len === 0) return '';
    // Note: Actual implementation requires FFI access to read memory.
    // This is a placeholder - the host/guest libraries provide actual implementation.
    return '';
}

/**
 * Strip a prefix from a string.
 * @param sv - The input StringView or string.
 * @param prefix - The prefix to strip.
 * @returns The string without prefix, or original if prefix not present.
 */
export function stripPrefix(sv: StringView | string, prefix: string): string {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    if (s.startsWith(prefix)) {
        return s.slice(prefix.length);
    }
    return s;
}

/**
 * Check if a string starts with a prefix.
 * @param sv - The input StringView or string.
 * @param prefix - The prefix to check.
 * @returns True if the string starts with the prefix.
 */
export function startsWith(sv: StringView | string, prefix: string): boolean {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    return s.startsWith(prefix);
}

/**
 * Check if a string ends with a suffix.
 * @param sv - The input StringView or string.
 * @param suffix - The suffix to check.
 * @returns True if the string ends with the suffix.
 */
export function endsWith(sv: StringView | string, suffix: string): boolean {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    return s.endsWith(suffix);
}

/**
 * Convert a StringView to a JavaScript string (shorthand alias).
 * @param sv - The StringView to convert.
 * @returns The JavaScript string, or empty string if null/empty.
 */
export function toStr(sv: StringView | null | undefined): string {
    return stringViewToString(sv);
}

/**
 * Split a string by a delimiter.
 * @param sv - The input StringView or string.
 * @param delimiter - The delimiter to split by.
 * @returns An array of strings.
 */
export function split(sv: StringView | string, delimiter: string): string[] {
    const s: string = typeof sv === 'string' ? sv : stringViewToString(sv);
    return s.split(delimiter);
}
"#;

/// C++ helper function definitions (no #pragma once, no #include directives).
/// Includes namespace wrappers since merge_cpp_helpers appends at end of file
/// and the generated abi.hpp has a closing namespace brace already.
const HELPER_CPP: &str = r#"

namespace polyplug {
namespace abi {

/// Convert StringView to std::string_view (zero-copy)
inline std::string_view to_string_view(StringView sv) noexcept {
    if (!sv.ptr || sv.len == 0) return {};
    return {reinterpret_cast<const char*>(sv.ptr), sv.len};
}

/// Convert StringView to std::string (copies data)
inline std::string to_string(StringView sv) {
    if (!sv.ptr || sv.len == 0) return {};
    return {reinterpret_cast<const char*>(sv.ptr), sv.len};
}

/// Convert StringView to std::string (alias for to_string)
inline std::string to_str(StringView sv) {
    return to_string(sv);
}

/// Strip prefix from a string.
/// @param sv The input StringView.
/// @param prefix The prefix to strip.
/// @return std::string_view without prefix if it starts with prefix, otherwise original.
inline std::string_view strip_prefix(StringView sv, std::string_view prefix) noexcept {
    auto s = to_string_view(sv);
    if (s.size() >= prefix.size() && s.substr(0, prefix.size()) == prefix) {
        return s.substr(prefix.size());
    }
    return s;
}

/// Check if string starts with prefix.
/// @param sv The input StringView.
/// @param prefix The prefix to check.
/// @return true if string starts with prefix.
inline bool starts_with(StringView sv, std::string_view prefix) noexcept {
    auto s = to_string_view(sv);
    return s.size() >= prefix.size() && s.substr(0, prefix.size()) == prefix;
}

/// Check if string ends with suffix.
/// @param sv The input StringView.
/// @param suffix The suffix to check.
/// @return true if string ends with suffix.
inline bool ends_with(StringView sv, std::string_view suffix) noexcept {
    auto s = to_string_view(sv);
    if (s.size() < suffix.size()) return false;
    return s.substr(s.size() - suffix.size()) == suffix;
}

/// Split string by delimiter.
/// @param sv The input StringView.
/// @param delimiter The delimiter character.
/// @return Vector of string_views.
inline std::vector<std::string_view> split(StringView sv, char delimiter) {
    auto s = to_string_view(sv);
    std::vector<std::string_view> result;
    size_t start = 0;

    for (size_t i = 0; i <= s.size(); ++i) {
        if (i == s.size() || s[i] == delimiter) {
            if (i > start) {
                result.push_back(s.substr(start, i - start));
            }
            start = i + 1;
        }
    }

    return result;
}

/// Create StringView from string literal (borrowed)
inline StringView string_view(const char* s) noexcept {
    return {reinterpret_cast<const uint8_t*>(s), std::strlen(s)};
}

/// Create StringView from std::string (borrowed - ensure string outlives view)
inline StringView string_view(const std::string& s) noexcept {
    return {reinterpret_cast<const uint8_t*>(s.data()), s.size()};
}

/// Create StringView from std::string_view (borrowed)
inline StringView string_view(std::string_view s) noexcept {
    return {reinterpret_cast<const uint8_t*>(s.data()), s.size()};
}

// NOTE: cross-boundary allocation (alloc_string) lives in the guest SDK
// (polyplug::alloc_string in guest.hpp), which routes through the stored
// HostApi. abi.hpp stays pure ABI with no link-time host dependency.

} // namespace abi
} // namespace polyplug
"#;

/// Known struct sizes from Rust layout.
///
/// MAINTENANCE: Update this table when Rust struct layouts change.
/// See polyplug_abi layout tests (test_*_size) for canonical sizes.
/// Each size is verified by `static_assert`/`ctypes.sizeof` in generated SDK files,
/// so a stale table causes layout test failures, not silent corruption.
const KNOWN_SIZES: &[(&str, usize)] = &[
    ("StringView", 16),
    ("Buffer", 24),
    ("CallArena", 40),
    ("ArenaOverflowBlock", 24),
    ("Version", 12),
    ("AbiError", 24),
    ("DependencyInfo", 24),
    ("DispatchMechanisms", 16),
    ("GuestContractInterface", 56),
    ("GuestContractInstance", 16),
    ("HostApi", 160),
    ("HostContractInterface", 80),
    ("HostContractInstance", 8),
    ("GuestContractHandle", 8),
    ("PluginDescriptor", 48),
    ("BundleInitContext", 24),
    ("RuntimeConfig", 32),
    ("ReloadPhase", 48),
    ("NativeDispatch", 16),
    ("VmDispatch", 16),
    ("VmLoaderData", 8),
];

/// Populate `size_hint` fields on `AbiStruct` entries using the known size table.
fn populate_size_hints(abi_types: &mut AbiTypes) {
    for struct_info in &mut abi_types.structs {
        if struct_info.size_hint.is_none() {
            for (name, size) in KNOWN_SIZES {
                if struct_info.name == *name {
                    struct_info.size_hint = Some(*size);
                    break;
                }
            }
        }
    }
}

/// Validate that all field types can be represented in target languages.
///
/// Per D-09: Build fails with clear error if a type cannot be represented.
fn validate_representable_types(abi_types: &AbiTypes) -> Result<(), String> {
    for struct_info in &abi_types.structs {
        for field in &struct_info.fields {
            for pattern in UNREPRESENTABLE_PATTERNS {
                if field.rust_type.contains(pattern) {
                    return Err(format!(
                        "Cannot represent type '{}' field '{}' with type '{}' in target languages. \
                         Consider simplifying the type or adding codegen support.",
                        struct_info.name, field.name, field.rust_type
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Auto-generated file header for each target language.
///
/// Per D-10: Every generated abi.* file starts with a header stating it is
/// auto-generated, with instructions about ast-grep preservation and manual
/// editing policy.
fn generate_auto_header(lang: TargetLang) -> String {
    match lang {
        TargetLang::Python => [
            "# THIS FILE IS AUTO-GENERATED BY polyplug_abi build script.",
            "# DO NOT EDIT STRUCT/FIELD DEFINITIONS.",
            "# Helper methods are preserved by ast-grep across regenerations.",
            "# To add methods, write them inside the class bodies -- they will be preserved.",
            "",
        ]
        .join("\n"),
        TargetLang::CSharp => [
            "// THIS FILE IS AUTO-GENERATED BY polyplug_abi build script.",
            "// DO NOT EDIT STRUCT/FIELD DEFINITIONS.",
            "// Helper methods are preserved by ast-grep across regenerations.",
            "",
        ]
        .join("\n"),
        TargetLang::Lua => [
            "-- THIS FILE IS AUTO-GENERATED BY polyplug_abi build script.",
            "-- DO NOT EDIT STRUCT/FIELD DEFINITIONS.",
            "-- Helper methods are preserved by ast-grep across regenerations.",
            "",
        ]
        .join("\n"),
        TargetLang::JavaScript => [
            "// THIS FILE IS AUTO-GENERATED BY polyplug_abi build script.",
            "// DO NOT EDIT STRUCT/FIELD DEFINITIONS.",
            "// Helper methods are preserved by ast-grep across regenerations.",
            "",
        ]
        .join("\n"),
        TargetLang::Cpp => [
            "// THIS FILE IS AUTO-GENERATED BY polyplug_abi build script.",
            "// DO NOT EDIT STRUCT/FIELD DEFINITIONS.",
            "// Helper methods are preserved by ast-grep across regenerations.",
            "",
        ]
        .join("\n"),
    }
}

/// Generate SDK for a specific language.
///
/// # Arguments
/// * `lang` - Target language.
/// * `abi_types` - Extracted ABI types.
///
/// # Returns
/// Generated SDK code as a string.
pub fn generate_language_sdk(lang: TargetLang, abi_types: &AbiTypes) -> String {
    let all_items: Vec<Item> = map_all_abi_types(&abi_types.types());

    let generator: Box<dyn CodeGenerator> = match lang {
        TargetLang::Cpp => Box::new(CppGenerator::new()),
        TargetLang::CSharp => Box::new(CSharpGenerator::new()),
        TargetLang::Python => Box::new(PythonGenerator::new()),
        TargetLang::Lua => Box::new(LuaGenerator::new()),
        TargetLang::JavaScript => Box::new(JsGenerator::new()),
    };

    // Map every enum name to its Rust `repr` so generators that reference enum
    // fields by their underlying integer type (Python ctypes) can size them.
    let enum_reprs: std::collections::HashMap<String, String> = all_items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(e) => Some((e.name.clone(), e.repr.clone())),
            _ => None,
        })
        .collect();

    let ctx: GenerationContext = GenerationContext::new().with_enum_reprs(enum_reprs);
    let mut output: String = String::new();

    // Prepend auto-generated header before the codegen header.
    output.push_str(&generate_auto_header(lang));
    output.push_str(&generator.generate_header(&ctx));

    // Languages whose bindings reference types eagerly at definition time
    // (C++ by-value fields, Python ctypes CFUNCTYPE/Structure references)
    // require every referenced aggregate to be defined before use.
    //
    // * C++ emits forward declarations and dependency-sorts definitions.
    // * Python has no forward-declaration mechanism for ctypes, so it relies
    //   solely on dependency-ordered emission.
    let emit_items: Vec<Item> = match lang {
        TargetLang::Cpp => {
            output.push_str(&cpp_forward_declarations(&all_items));
            cpp_dependency_ordered(all_items)
        }
        TargetLang::Python => python_dependency_ordered(all_items),
        TargetLang::Lua => {
            // LuaJIT's cdef parser requires every aggregate to be declared
            // before it is referenced (in a typedef, by value, or by pointer).
            // Emit forward declarations for all aggregates so function-pointer
            // typedefs and pointer fields may reference any type, then
            // dependency-sort the definitions so every by-value field
            // references an already-completed type.
            output.push_str(&lua_forward_declarations(&all_items));
            lua_dependency_ordered(all_items)
        }
        _ => all_items,
    };

    // Lua emits all C declarations inside a single `ffi.cdef[[ ... ]]` block,
    // while constants are plain Lua statements (`M.X = ffi.cast(...)`) that must
    // live OUTSIDE that block. Emit aggregates first, close the cdef block, then
    // emit constants in module scope.
    let lua_consts: &mut Vec<&Item> = &mut Vec::new();

    for item in &emit_items {
        if lang == TargetLang::Lua && matches!(item, Item::Const(_)) {
            lua_consts.push(item);
            continue;
        }
        let code: String = match item {
            Item::Const(c) => generator.generate_const(c, &ctx),
            Item::Struct(s) => generator.generate_struct(s, &ctx),
            Item::Enum(e) => generator.generate_enum(e, &ctx),
            Item::Union(u) => generator.generate_union(u, &ctx),
            // Function items are no longer generated from ABI extraction.
            // The Function variant remains in codegen for use by polyplugc CLI.
            Item::Function(_) => String::new(),
        };
        output.push_str(&code);
    }

    if lang == TargetLang::Lua {
        // Close the ffi.cdef block opened by the header before emitting Lua
        // statements (constants) in module scope.
        output.push_str("]]\n\n");
        for item in lua_consts.iter() {
            if let Item::Const(c) = item {
                output.push_str(&generator.generate_const(c, &ctx));
            }
        }
        output.push('\n');
    }

    output.push_str(&generator.generate_footer(&ctx));
    output
}

/// Emit C++ forward declarations for every struct, union, and enum item.
///
/// Forward declarations resolve all pointer and function-pointer references,
/// leaving only by-value field dependencies for `cpp_dependency_ordered` to
/// satisfy via emission order.
fn cpp_forward_declarations(items: &[Item]) -> String {
    let mut output = String::from("// ─── Forward declarations ───\n");
    for item in items {
        let decl = match item {
            Item::Struct(s) => CppGenerator::forward_declaration(&s.name, ForwardKind::Struct),
            Item::Union(u) => CppGenerator::forward_declaration(&u.name, ForwardKind::Union),
            Item::Enum(e) => {
                CppGenerator::forward_declaration(&e.name, ForwardKind::Enum(e.repr.clone()))
            }
            Item::Const(_) | Item::Function(_) => continue,
        };
        output.push_str(&decl);
    }
    output.push('\n');
    output
}

/// Emit LuaJIT-cdef forward declarations for every struct, union, and enum.
///
/// LuaJIT's cdef parser is not lazy: a type must be declared before it is named
/// in a typedef, by value, or by pointer. A forward `typedef struct X X;` (and
/// the enum/union equivalents) declares the tag and aliases it so any later
/// reference resolves, and the subsequent combined `typedef struct X { ... } X;`
/// completes the type without conflict.
fn lua_forward_declarations(items: &[Item]) -> String {
    let mut output = String::from("    // ─── Forward declarations ───\n");
    for item in items {
        let decl: String = match item {
            Item::Struct(s) => format!("    typedef struct {} {};\n", s.name, s.name),
            Item::Union(u) => format!("    typedef union {} {};\n", u.name, u.name),
            Item::Enum(e) => format!("    typedef enum {} {};\n", e.name, e.name),
            Item::Const(_) | Item::Function(_) => continue,
        };
        output.push_str(&decl);
    }
    output.push('\n');
    output
}

/// Order LuaJIT-cdef items so every by-value field references an
/// already-completed type.
///
/// Forward declarations (emitted by `lua_forward_declarations`) resolve all
/// pointer and function-pointer references, leaving only by-value struct,
/// union, and enum fields to satisfy via emission order. Constants and
/// definitions with satisfied dependencies keep their relative source order.
fn lua_dependency_ordered(items: Vec<Item>) -> Vec<Item> {
    use std::collections::HashSet;

    // Aggregates that can complete a by-value dependency: structs, unions, and
    // enums (an enum field needs its size, i.e. its definition, first).
    let aggregate_names: HashSet<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) => Some(s.name.clone()),
            Item::Union(u) => Some(u.name.clone()),
            Item::Enum(e) => Some(e.name.clone()),
            _ => None,
        })
        .collect();

    let dependencies = |item: &Item| -> Vec<String> {
        let field_types: Vec<&str> = match item {
            Item::Struct(s) => s.fields.iter().map(|f| f.rust_type.as_str()).collect(),
            Item::Union(u) => u.variants.iter().map(|v| v.type_name.as_str()).collect(),
            _ => Vec::new(),
        };
        field_types
            .iter()
            .filter_map(|rust_type| LuaGenerator::value_dependency(rust_type))
            .filter(|dep| aggregate_names.contains(dep))
            .collect()
    };

    let mut ordered: Vec<Item> = Vec::with_capacity(items.len());
    let mut emitted: HashSet<String> = HashSet::new();
    let mut pending: Vec<Item> = items;

    loop {
        let mut progressed = false;
        let mut next_pending: Vec<Item> = Vec::new();

        for item in pending {
            let name: Option<String> = match &item {
                Item::Struct(s) => Some(s.name.clone()),
                Item::Union(u) => Some(u.name.clone()),
                Item::Enum(e) => Some(e.name.clone()),
                _ => None,
            };

            let ready: bool = dependencies(&item).iter().all(|dep| emitted.contains(dep));
            if ready {
                if let Some(name) = name {
                    emitted.insert(name);
                }
                ordered.push(item);
                progressed = true;
            } else {
                next_pending.push(item);
            }
        }

        if next_pending.is_empty() {
            break;
        }
        if !progressed {
            // A dependency cycle (or unresolved dependency) remains. Emit the
            // rest in source order rather than dropping items.
            ordered.extend(next_pending);
            break;
        }
        pending = next_pending;
    }

    ordered
}

/// Order C++ items so that every by-value field dependency is defined before
/// the struct or union that uses it.
///
/// Enums and constants carry no aggregate dependencies and keep their relative
/// order. Structs and unions are topologically sorted by their by-value field
/// dependencies (resolved via `CppGenerator::value_dependency`).
fn cpp_dependency_ordered(items: Vec<Item>) -> Vec<Item> {
    use std::collections::HashSet;

    // Names of aggregates whose definitions still need to be emitted.
    let aggregate_names: HashSet<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) => Some(s.name.clone()),
            Item::Union(u) => Some(u.name.clone()),
            _ => None,
        })
        .collect();

    // By-value dependencies for each struct/union, restricted to aggregates we
    // are emitting (enums are satisfied by their forward declaration).
    let dependencies = |item: &Item| -> Vec<String> {
        let fields: Vec<&str> = match item {
            Item::Struct(s) => s.fields.iter().map(|f| f.rust_type.as_str()).collect(),
            Item::Union(u) => u.variants.iter().map(|v| v.type_name.as_str()).collect(),
            _ => Vec::new(),
        };
        fields
            .iter()
            .filter_map(|rust_type| CppGenerator::value_dependency(rust_type))
            .filter(|dep| aggregate_names.contains(dep))
            .collect()
    };

    let mut ordered: Vec<Item> = Vec::with_capacity(items.len());
    let mut emitted: HashSet<String> = HashSet::new();
    let mut pending: Vec<Item> = items;

    // Stable topological emission: repeatedly emit every item whose aggregate
    // dependencies are already emitted. Non-aggregate items and aggregates with
    // satisfied dependencies flush in source order each pass.
    loop {
        let mut progressed = false;
        let mut next_pending: Vec<Item> = Vec::new();

        for item in pending {
            let name = match &item {
                Item::Struct(s) => Some(s.name.clone()),
                Item::Union(u) => Some(u.name.clone()),
                _ => None,
            };

            let ready = dependencies(&item).iter().all(|dep| emitted.contains(dep));
            if ready {
                if let Some(name) = name {
                    emitted.insert(name);
                }
                ordered.push(item);
                progressed = true;
            } else {
                next_pending.push(item);
            }
        }

        if next_pending.is_empty() {
            break;
        }
        if !progressed {
            // A dependency cycle (or unresolved dependency) remains. Emit the
            // rest in source order rather than dropping items.
            ordered.extend(next_pending);
            break;
        }
        pending = next_pending;
    }

    ordered
}

/// Order Python items so that every name referenced eagerly by ctypes is
/// defined before use.
///
/// Python's ctypes has no forward-declaration mechanism: a `Structure`
/// `_fields_` entry referencing another aggregate, and a `CFUNCTYPE` typedef
/// referencing its return/parameter types, all evaluate those names at
/// definition time. Constants carry no dependencies and keep their relative
/// order. Structs, enums, and unions are topologically sorted by the named
/// aggregates they reference by value (resolved via
/// `PythonGenerator::type_dependencies`). Pointer fields impose no constraint.
fn python_dependency_ordered(items: Vec<Item>) -> Vec<Item> {
    use std::collections::HashSet;

    let aggregate_names: HashSet<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) => Some(s.name.clone()),
            Item::Enum(e) => Some(e.name.clone()),
            Item::Union(u) => Some(u.name.clone()),
            _ => None,
        })
        .collect();

    let dependencies = |item: &Item| -> Vec<String> {
        let field_types: Vec<&str> = match item {
            Item::Struct(s) => s.fields.iter().map(|f| f.rust_type.as_str()).collect(),
            Item::Union(u) => u.variants.iter().map(|v| v.type_name.as_str()).collect(),
            _ => Vec::new(),
        };
        field_types
            .iter()
            .flat_map(|rust_type| PythonGenerator::type_dependencies(rust_type))
            .filter(|dep| aggregate_names.contains(dep))
            .collect()
    };

    let mut ordered: Vec<Item> = Vec::with_capacity(items.len());
    let mut emitted: HashSet<String> = HashSet::new();
    let mut pending: Vec<Item> = items;

    // Stable topological emission: repeatedly emit every item whose aggregate
    // dependencies are already emitted. Constants and aggregates with satisfied
    // dependencies flush in source order each pass.
    loop {
        let mut progressed = false;
        let mut next_pending: Vec<Item> = Vec::new();

        for item in pending {
            let name: Option<String> = match &item {
                Item::Struct(s) => Some(s.name.clone()),
                Item::Enum(e) => Some(e.name.clone()),
                Item::Union(u) => Some(u.name.clone()),
                _ => None,
            };

            let ready: bool = dependencies(&item).iter().all(|dep| emitted.contains(dep));
            if ready {
                if let Some(name) = name {
                    emitted.insert(name);
                }
                ordered.push(item);
                progressed = true;
            } else {
                next_pending.push(item);
            }
        }

        if next_pending.is_empty() {
            break;
        }
        if !progressed {
            // A dependency cycle (or unresolved dependency) remains. Emit the
            // rest in source order rather than dropping items.
            ordered.extend(next_pending);
            break;
        }
        pending = next_pending;
    }

    ordered
}

impl TargetLang {
    /// Return files in the abi directory that should be deleted before regeneration
    /// (the generated abi.* file itself).
    fn generated_filenames(&self) -> Vec<&'static str> {
        vec![self.output_filename()]
    }
}

/// Return inline helper method content for a given language.
///
/// Per D-12: Helper methods are embedded as const strings so they survive
/// across consecutive rebuilds without relying on external helper files.
fn get_inline_helpers(lang: TargetLang) -> Vec<(String, String)> {
    match lang {
        TargetLang::CSharp => vec![
            (
                "StringViewHelper.cs".to_string(),
                HELPER_CSHARP_STRING_VIEW.to_string(),
            ),
            (
                "StringHelpers.cs".to_string(),
                HELPER_CSHARP_STRING_HELPERS.to_string(),
            ),
        ],
        TargetLang::Lua => vec![
            ("string_view_helper.lua".to_string(), HELPER_LUA.to_string()),
            ("hashing.lua".to_string(), HELPER_LUA_HASHING.to_string()),
        ],
        TargetLang::JavaScript => {
            vec![("string_view_helper.ts".to_string(), HELPER_JS.to_string())]
        }
        TargetLang::Cpp => vec![("string_view_helper.hpp".to_string(), HELPER_CPP.to_string())],
        TargetLang::Python => vec![("hashing.py".to_string(), HELPER_PYTHON_HASHING.to_string())],
    }
}

/// Delete old generated abi.* files before writing fresh ones.
///
/// Per D-11: Delete all broken/old abi.* files before codegen writes fresh ones.
/// Helper files are NOT deleted here -- they are consumed by the merge step.
fn delete_old_generated_files(lang: TargetLang, abi_dir: &Path) {
    for filename in lang.generated_filenames() {
        let path = abi_dir.join(filename);
        if path.exists() {
            if let Err(e) = fs::remove_file(&path) {
                println!(
                    "cargo:warning=Failed to delete old file {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }
}

/// Strip the auto-generated header from helper file contents.
///
/// Helper files may have their own "AUTO-GENERATED" headers that should be
/// removed when merging, since the merged file has its own header.
fn strip_auto_generated_header(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut start = 0;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("// THIS FILE IS AUTO-GENERATED")
            || trimmed.starts_with("-- THIS FILE IS AUTO-GENERATED")
            || trimmed.starts_with("# THIS FILE IS AUTO-GENERATED")
            || trimmed.starts_with("// DO NOT EDIT")
            || trimmed.starts_with("-- DO NOT EDIT")
            || trimmed.starts_with("# DO NOT EDIT")
            || (trimmed.is_empty() && start == i)
        {
            start = i + 1;
            continue;
        }
        // Stop stripping once we hit real content (non-header lines).
        if !trimmed.is_empty()
            && !trimmed.starts_with("///")
            && !trimmed.starts_with("/**")
            && !trimmed.starts_with("// @")
            && !trimmed.starts_with("-- @")
            && !trimmed.starts_with("* @")
            && !trimmed.starts_with(" *")
        {
            break;
        }
    }
    lines[start..].join("\n")
}

/// Extract method/function bodies from a Lua helper file using regex.
///
/// Per D-14 research: ast-grep has limited Lua support, so we use a simple
/// regex-based extractor for Lua helper files. Looks for `function` patterns
/// that define methods on the module table.
fn extract_lua_helper_methods(content: &str) -> String {
    let mut methods = Vec::new();
    let mut in_function = false;
    let mut depth = 0;
    let mut current = String::new();

    for line in content.lines() {
        if !in_function {
            // Detect function start: `function M.name(...)` or `function M.name(`
            let trimmed = line.trim();
            if trimmed.starts_with("function M.") || trimmed.starts_with("function M ") {
                in_function = true;
                depth = 0;
                current.clear();
                current.push_str(line);
                current.push('\n');
                // Count opening/closing keywords for depth tracking
                depth += count_lua_openers(trimmed);
            }
        } else {
            current.push_str(line);
            current.push('\n');
            let trimmed = line.trim();
            depth += count_lua_openers(trimmed);
            if trimmed == "end" || trimmed.starts_with("end ") || trimmed.starts_with("end--") {
                depth = depth.saturating_sub(1);
            }
            if depth == 0 {
                methods.push(current.trim().to_string());
                current.clear();
                in_function = false;
            }
        }
    }

    if in_function && !current.trim().is_empty() {
        methods.push(current.trim().to_string());
    }

    methods.join("\n\n")
}

/// Count Lua block-opening keywords in a line.
///
/// # Limitations
/// Only tracks keywords at the start of a line (`starts_with`). Nested constructs
/// where keywords appear mid-line are not tracked. This is sufficient for the
/// inline helper methods in `HELPER_LUA`, which are simple top-level functions.
fn count_lua_openers(line: &str) -> i32 {
    let mut count = 0i32;
    let trimmed = line.trim();
    if trimmed.starts_with("function ") || trimmed.starts_with("function(") {
        count += 1;
    }
    if trimmed.starts_with("if ") || trimmed == "if" {
        count += 1;
    }
    if trimmed.starts_with("for ") || trimmed == "for" {
        count += 1;
    }
    if trimmed.starts_with("while ") || trimmed == "while" {
        count += 1;
    }
    // `end` at EOL doesn't count as opener, but `then` is part of `if`
    if trimmed.contains(" do") || trimmed.ends_with(" do") {
        count += 1;
    }
    if trimmed.contains(" then") || trimmed.ends_with(" then") {
        // `if ... then` already counted above, but elseif needs extra
    }
    count
}

/// Merge helper file contents into the generated code for a specific language.
///
/// Per D-12: Helper files (StringViewHelper.cs, string_view_helper.lua, etc.)
/// merge into abi.* files. The helper methods are appended at the end of the
/// generated file in a language-appropriate location.
fn merge_helpers_into_generated(
    lang: TargetLang,
    generated_code: &str,
    helpers: &[(String, String)],
) -> String {
    if helpers.is_empty() {
        return generated_code.to_string();
    }

    match lang {
        TargetLang::CSharp => merge_csharp_helpers(generated_code, helpers),
        TargetLang::Lua => merge_lua_helpers(generated_code, helpers),
        TargetLang::JavaScript => merge_js_helpers(generated_code, helpers),
        TargetLang::Cpp => merge_cpp_helpers(generated_code, helpers),
        TargetLang::Python => merge_python_helpers(generated_code, helpers),
    }
}

/// Merge Python helper functions into the generated abi.py.
///
/// The helper content is module-level Python (constants + functions). It is
/// appended after the generated type definitions; import lines are stripped
/// because all required imports already live at the top of the generated file.
fn merge_python_helpers(generated_code: &str, helpers: &[(String, String)]) -> String {
    let mut result: String = generated_code.to_string();
    result.push_str("\n\n# ─── Helper Methods (preserved from helper files) ───\n");

    for (_filename, contents) in helpers {
        let cleaned: String = strip_auto_generated_header(contents);
        let trimmed: &str = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }
        let body: String = trimmed
            .lines()
            .filter(|line| {
                let lt: &str = line.trim();
                !lt.starts_with("import ") && !lt.starts_with("from ")
            })
            .collect::<Vec<&str>>()
            .join("\n");
        result.push('\n');
        result.push_str(&body);
        result.push('\n');
    }

    result
}

/// Merge C# helper classes into the generated Abi.cs namespace.
///
/// The helper files contain static classes like StringViewHelper and StringHelpers.
/// They are appended inside the namespace block before the closing brace.
fn merge_csharp_helpers(generated_code: &str, helpers: &[(String, String)]) -> String {
    let mut merged = generated_code.to_string();

    // Find the last closing brace of the namespace block.
    // C# generated code ends with "}\n" for the namespace.
    if let Some(pos) = merged.rfind('}') {
        let mut helper_block =
            String::from("\n// ─── Helper Methods (preserved from helper files) ───\n\n");

        for (_filename, contents) in helpers {
            let cleaned = strip_auto_generated_header(contents);
            let trimmed = cleaned.trim();
            if !trimmed.is_empty() {
                // The helper classes use `namespace Polyplug.Abi;` or
                // `namespace Polyplug.Abi` with braces. We need to strip
                // the namespace wrapper and `using` statements that are
                // already in the generated file.
                let body = extract_csharp_class_body(trimmed);
                helper_block.push_str(&body);
                helper_block.push('\n');
            }
        }

        merged.insert_str(pos, &helper_block);
    }

    merged
}

/// Extract the class/struct body from a C# helper file, removing namespace
/// wrappers and using statements that duplicate the generated file.
fn extract_csharp_class_body(content: &str) -> String {
    let mut result = String::new();
    let mut in_namespace_brace = false;
    let mut brace_depth = 0;
    let mut skip_block = false;
    let mut using_lines = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Collect using statements separately.
        if trimmed.starts_with("using ") && !in_namespace_brace {
            // Only include usings not already in generated code
            if !trimmed.contains("System.Runtime.InteropServices")
                && !trimmed.contains("System.Text")
            {
                using_lines.push_str(line);
                using_lines.push('\n');
            }
            continue;
        }

        // Skip namespace declaration lines.
        if trimmed.starts_with("namespace ") {
            if trimmed.ends_with('{') {
                in_namespace_brace = true;
                brace_depth = 1;
            }
            // file-scoped namespace (ends with ;) -- skip, body follows
            continue;
        }

        if in_namespace_brace {
            // Count braces to find end of namespace
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            in_namespace_brace = false;
                            skip_block = true;
                        }
                    }
                    _ => {}
                }
            }
            if skip_block {
                skip_block = false;
                continue;
            }
        }

        // Skip empty lines at start of content (before class definition)
        if result.is_empty() && trimmed.is_empty() {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    let body = result.trim();
    if body.is_empty() {
        return String::new();
    }

    // Prepend any extra using statements needed by helpers
    if using_lines.trim().is_empty() {
        body.to_string()
    } else {
        format!("{}\n{}", using_lines.trim(), body)
    }
}

/// Merge Lua helper functions into the generated abi.lua module.
///
/// The generated abi.lua has structure:
///   local ffi = require("ffi")
///   local M = {}
///   <ffi.cdef typedefs>
///   M.CONST = value
///   return M
///
/// Helper functions like `function M.to_str(sv)` are appended before `return M`.
fn merge_lua_helpers(generated_code: &str, helpers: &[(String, String)]) -> String {
    let mut helper_block = String::new();
    helper_block.push_str("\n-- ─── Helper Methods (preserved from helper files) ───\n\n");

    for (_filename, contents) in helpers {
        let cleaned = strip_auto_generated_header(contents);
        // Extract only the function definitions (skip module boilerplate)
        let methods = extract_lua_helper_methods(&cleaned);
        if !methods.trim().is_empty() {
            helper_block.push_str(&methods);
            helper_block.push_str("\n\n");
        }
    }

    // Insert before "return M" at the end
    if let Some(pos) = generated_code.rfind("return M") {
        let mut result = generated_code[..pos].to_string();
        result.push_str(&helper_block);
        result.push_str("return M\n");
        result
    } else {
        let mut result = generated_code.to_string();
        result.push_str(&helper_block);
        result
    }
}

/// Merge JS/TS helper functions into the generated abi.ts.
///
/// The helper file contains exported functions that are appended after
/// the generated type definitions and constants.
fn merge_js_helpers(generated_code: &str, helpers: &[(String, String)]) -> String {
    let mut result = generated_code.to_string();
    result.push_str("\n// ─── Helper Methods (preserved from helper files) ───\n\n");

    for (_filename, contents) in helpers {
        let cleaned = strip_auto_generated_header(contents);
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            // Strip import lines since types are in the same file now
            let body: String = trimmed
                .lines()
                .filter(|line| {
                    let lt = line.trim();
                    !lt.starts_with("import ")
                })
                .collect::<Vec<&str>>()
                .join("\n");
            result.push_str(&body);
            result.push_str("\n\n");
        }
    }

    result
}

/// Merge C++ helper functions into the generated abi.hpp.
///
/// The helper file contains inline functions in the polyplug::abi namespace.
/// They are appended at the end of the generated header, inside the namespace.
fn merge_cpp_helpers(generated_code: &str, helpers: &[(String, String)]) -> String {
    let mut result = generated_code.to_string();
    result.push_str("\n// ─── Helper Methods (preserved from helper files) ───\n");

    for (_filename, contents) in helpers {
        let cleaned = strip_auto_generated_header(contents);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Strip include directives and pragma once (already in generated file)
        let body: String = trimmed
            .lines()
            .filter(|line| {
                let lt = line.trim();
                !lt.starts_with("#pragma once")
                    && !lt.starts_with("#include \"abi.hpp\"")
                    && !lt.starts_with("#include <cstring>")
                    && !lt.starts_with("#include <string>")
                    && !lt.starts_with("#include <string_view>")
                    && !lt.starts_with("#include <vector>")
            })
            .collect::<Vec<&str>>()
            .join("\n");

        result.push_str(&body);
        result.push('\n');
    }

    result
}

/// Generate all SDKs and write to sdks/{lang}/abi/.
///
/// # Arguments
/// * `abi_types` - Extracted ABI types (will be mutated to populate size hints).
/// * `workspace_root` - Path to the workspace root directory.
/// * `tracked_files` - Source files to emit `cargo:rerun-if-changed` for.
///
/// # Returns
/// Result indicating success or failure.
pub fn generate_all_sdks(
    abi_types: &mut AbiTypes,
    workspace_root: &Path,
    tracked_files: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    // Populate size hints from known size table.
    populate_size_hints(abi_types);

    // Validate that all types can be represented in target languages (D-09).
    validate_representable_types(abi_types)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    // Emit cargo:rerun-if-changed for all tracked source files.
    for path in tracked_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let languages: [TargetLang; 5] = [
        TargetLang::Cpp,
        TargetLang::CSharp,
        TargetLang::Python,
        TargetLang::Lua,
        TargetLang::JavaScript,
    ];

    for lang in languages {
        let abi_dir: PathBuf = workspace_root
            .join("sdks")
            .join(lang.language_name())
            .join("abi");

        // ── Step 1: Get inline helper method content (D-12) ──
        let helpers = get_inline_helpers(lang);

        // ── Step 2: Delete old generated abi.* files (D-11) ──
        delete_old_generated_files(lang, &abi_dir);

        // ── Step 3: Generate fresh code ──
        let mut sdk: String = generate_language_sdk(lang, abi_types);

        // ── Step 4: Merge helper methods into generated output (D-12) ──
        sdk = merge_helpers_into_generated(lang, &sdk, &helpers);

        let output_path: PathBuf = if lang.subdir().is_empty() {
            abi_dir.join(lang.output_filename())
        } else {
            abi_dir.join(lang.subdir()).join(lang.output_filename())
        };

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&output_path, sdk)?;
    }

    // Generate layout test source files per D-31.
    generate_layout_tests(abi_types, workspace_root)?;

    Ok(())
}

/// Generate layout test source files for all SDK languages per D-31.
///
/// Per D-32: Only generates test source files. Test scaffolding (project files,
/// conftest) must be created manually per SDK.
fn generate_layout_tests(
    abi_types: &AbiTypes,
    workspace_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Collect structs with known sizes.
    let sized_structs: Vec<(&str, usize)> = abi_types
        .structs
        .iter()
        .filter_map(|s| s.size_hint.map(|size| (s.name.as_str(), size)))
        .collect();

    if sized_structs.is_empty() {
        return Ok(());
    }

    // Python: test_layout.py with pytest assertions.
    let python_tests = generate_python_layout_tests(&sized_structs);
    let python_dir = workspace_root.join("sdks/python/abi");
    std::fs::create_dir_all(&python_dir)?;
    std::fs::write(python_dir.join("test_layout.py"), python_tests)?;

    // C#: LayoutTests.cs with xUnit. Written to the dedicated test project so
    // the shipped Polyplug.Abi library does not glob-compile an xunit-dependent file.
    let csharp_tests = generate_csharp_layout_tests(&sized_structs);
    let csharp_dir = workspace_root.join("sdks/csharp/abi.tests");
    std::fs::create_dir_all(&csharp_dir)?;
    std::fs::write(csharp_dir.join("LayoutTests.cs"), csharp_tests)?;

    // Lua: test_layout.lua with simple assertions.
    let lua_tests = generate_lua_layout_tests(&sized_structs);
    let lua_dir = workspace_root.join("sdks/lua/abi");
    std::fs::create_dir_all(&lua_dir)?;
    std::fs::write(lua_dir.join("test_layout.lua"), lua_tests)?;

    // JS: test_layout.ts with Deno.test.
    let js_tests = generate_js_layout_tests(&sized_structs);
    let js_dir = workspace_root.join("sdks/js/abi");
    std::fs::create_dir_all(&js_dir)?;
    std::fs::write(js_dir.join("test_layout.ts"), js_tests)?;

    // C++: test_layout.cpp with static_assert.
    let cpp_tests = generate_cpp_layout_tests(&sized_structs);
    let cpp_dir = workspace_root.join("sdks/cpp/abi");
    std::fs::create_dir_all(&cpp_dir)?;
    std::fs::write(cpp_dir.join("test_layout.cpp"), cpp_tests)?;

    Ok(())
}

/// Generate Python layout test file content.
fn generate_python_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("# Layout tests for polyplug ABI structs.\n");
    output.push_str("# AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("import ctypes\n\n");

    // Import all structs from the generated abi module.
    output.push_str("from abi import (\n");
    for (name, _) in sized_structs {
        output.push_str(&format!("    {},\n", name));
    }
    output.push_str(")\n\n\n");

    for (name, size) in sized_structs {
        let test_name = to_snake_case(name);
        output.push_str(&format!(
            "def test_{}_size():\n    assert ctypes.sizeof({}) == {}, \
             f\"{} expected {} bytes, got {{ctypes.sizeof({})}}\"\n\n\n",
            test_name, name, size, name, size, name
        ));
    }

    output
}

/// Generate C# layout test file content.
fn generate_csharp_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("// Layout tests for polyplug ABI structs.\n");
    output.push_str("// AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("using System.Runtime.InteropServices;\n");
    output.push_str("using Xunit;\n\n");
    output.push_str("namespace Polyplug.Abi.Tests\n{\n");
    output.push_str("    public class LayoutTests\n    {\n");

    for (name, size) in sized_structs {
        let test_name = format!("{}Is{}Bytes", name, size);
        output.push_str(&format!(
            "        [Fact]\n        public void {}() => \
             Assert.Equal({}, Marshal.SizeOf<{}>());\n\n",
            test_name, size, name
        ));
    }

    output.push_str("    }\n}\n");
    output
}

/// Generate Lua layout test file content.
fn generate_lua_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("-- Layout tests for polyplug ABI structs.\n");
    output.push_str("-- AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    // Resolve sibling modules (abi.lua) when run standalone from any directory,
    // then load abi.lua so its `ffi.cdef` struct declarations are registered
    // before `ffi.sizeof` is called. Without this, `ffi.sizeof("NativeDispatch")`
    // fails with "declaration specifier expected" — the types are undeclared.
    output.push_str(
        "local script_dir = (arg and arg[0] or \"\"):match(\"^(.*[/\\\\])\") or \"./\"\n",
    );
    output.push_str("package.path = script_dir .. \"?.lua;\" .. package.path\n");
    output.push_str("local ffi = require(\"ffi\")\n");
    output.push_str("require(\"abi\")\n\n");

    for (name, size) in sized_structs {
        output.push_str(&format!(
            "assert(ffi.sizeof(\"{}\") == {}, \"{} size mismatch\")\n",
            name, size, name
        ));
    }

    output.push_str("\nprint(\"All layout tests passed!\")\n");
    output
}

/// Generate JS/TS layout test file content.
fn generate_js_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("// Layout tests for polyplug ABI structs.\n");
    output.push_str("// AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("import {\n");
    for (name, _) in sized_structs {
        output.push_str(&format!(
            "    {}_SIZE,\n",
            to_upper_snake_case_for_generate(name)
        ));
    }
    output.push_str("} from \"./abi.ts\";\n");
    output.push_str("import { assert } from \"jsr:@std/assert\";\n\n");

    for (name, size) in sized_structs {
        let const_name = format!("{}_SIZE", to_upper_snake_case_for_generate(name));
        output.push_str(&format!(
            "Deno.test(\"{} is {} bytes\", () => {{\n    assert({} === {});\n}});\n\n",
            name, size, const_name, size
        ));
    }

    output
}

/// Generate C++ layout test file content.
fn generate_cpp_layout_tests(sized_structs: &[(&str, usize)]) -> String {
    let mut output = String::new();
    output.push_str("// Layout tests for polyplug ABI structs.\n");
    output.push_str("// AUTO-GENERATED by polyplug_abi build script — do not edit.\n\n");
    output.push_str("#include \"polyplug/abi.hpp\"\n\n");

    for (name, size) in sized_structs {
        output.push_str(&format!(
            "static_assert(sizeof({}) == {}, \"{} size mismatch\");\n",
            name, size, name
        ));
    }

    output
}

/// Convert PascalCase to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert PascalCase to UPPER_SNAKE_CASE for JS constants.
///
/// Handles consecutive uppercase letters correctly:
/// `AbiError` -> `ABI_ERROR`, not `A_B_I_E_R_R_O_R`.
fn to_upper_snake_case_for_generate(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            // Insert underscore at boundaries:
            // - Before uppercase if previous was lowercase (e.g., `aB` -> `a_B`)
            // - Before uppercase if next is lowercase and we have a run of uppercase
            //   (e.g., `ABIError` -> `ABI_Error`)
            if i > 0 {
                let prev = chars[i - 1];
                if prev.is_ascii_lowercase()
                    || (prev.is_uppercase()
                        && i + 1 < chars.len()
                        && chars[i + 1].is_ascii_lowercase())
                {
                    result.push('_');
                }
            }
            result.push(*c);
        } else {
            result.push(c.to_ascii_uppercase());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::types::{AbiConst, AbiStruct};

    #[test]
    fn test_target_lang_language_name() {
        assert_eq!(TargetLang::Cpp.language_name(), "cpp");
        assert_eq!(TargetLang::CSharp.language_name(), "csharp");
        assert_eq!(TargetLang::Python.language_name(), "python");
        assert_eq!(TargetLang::Lua.language_name(), "lua");
        assert_eq!(TargetLang::JavaScript.language_name(), "js");
    }

    #[test]
    fn test_generate_language_sdk_cpp() {
        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_const(AbiConst {
            name: String::from("TEST_CONST"),
            rust_type: String::from("u32"),
            value: String::from("42"),
            doc: Some(String::from("Test constant.")),
        });

        let sdk: String = generate_language_sdk(TargetLang::Cpp, &abi_types);

        assert!(sdk.contains("#pragma once"));
        assert!(sdk.contains("#include <cstdint>"));
        assert!(sdk.contains("TEST_CONST"));
    }

    #[test]
    fn test_generate_language_sdk_python() {
        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_const(AbiConst {
            name: String::from("TEST_CONST"),
            rust_type: String::from("u32"),
            value: String::from("42"),
            doc: Some(String::from("Test constant.")),
        });

        let sdk: String = generate_language_sdk(TargetLang::Python, &abi_types);

        assert!(sdk.contains("import ctypes"));
        assert!(sdk.contains("TEST_CONST"));
    }

    /// Test that populate_size_hints fills in known struct sizes.
    #[test]
    fn test_populate_size_hints() {
        use crate::build::types::AbiField;

        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_struct(AbiStruct {
            name: String::from("RuntimeConfig"),
            fields: vec![],
            doc: None,
            repr_c: true,
            size_hint: None,
        });
        abi_types.add_struct(AbiStruct {
            name: String::from("GuestContractHandle"),
            fields: vec![],
            doc: None,
            repr_c: true,
            size_hint: None,
        });
        abi_types.add_struct(AbiStruct {
            name: String::from("UnknownStruct"),
            fields: vec![],
            doc: None,
            repr_c: true,
            size_hint: None,
        });

        populate_size_hints(&mut abi_types);

        assert_eq!(
            abi_types.structs[0].size_hint,
            Some(16),
            "RuntimeConfig should be 16 bytes"
        );
        assert_eq!(
            abi_types.structs[1].size_hint,
            Some(8),
            "GuestContractHandle should be 8 bytes"
        );
        assert_eq!(
            abi_types.structs[2].size_hint, None,
            "Unknown struct should have no size hint"
        );
    }

    /// Test that C++ output contains static_assert for structs with size hints.
    #[test]
    fn test_cpp_output_contains_static_assert() {
        use crate::build::types::AbiField;

        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_struct(AbiStruct {
            name: String::from("RuntimeConfig"),
            fields: vec![AbiField {
                name: String::from("compatibility"),
                rust_type: String::from("u32"),
                doc: None,
            }],
            doc: None,
            repr_c: true,
            size_hint: Some(16),
        });

        let sdk: String = generate_language_sdk(TargetLang::Cpp, &abi_types);
        assert!(
            sdk.contains("static_assert(sizeof(RuntimeConfig) == 16"),
            "C++ should contain static_assert for RuntimeConfig: {}",
            sdk
        );
    }

    /// Test that Python output contains ctypes.sizeof assertions for structs with size hints.
    #[test]
    fn test_python_output_contains_sizeof_assertions() {
        use crate::build::types::AbiField;

        let mut abi_types: AbiTypes = AbiTypes::new();
        abi_types.add_struct(AbiStruct {
            name: String::from("RuntimeConfig"),
            fields: vec![AbiField {
                name: String::from("compatibility"),
                rust_type: String::from("u32"),
                doc: None,
            }],
            doc: None,
            repr_c: true,
            size_hint: Some(16),
        });

        let sdk: String = generate_language_sdk(TargetLang::Python, &abi_types);
        assert!(
            sdk.contains("assert ctypes.sizeof(RuntimeConfig) == 16"),
            "Python should contain ctypes.sizeof assertion for RuntimeConfig: {}",
            sdk
        );
    }
}
