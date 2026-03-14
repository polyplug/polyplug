# pyright: reportMissingImports=false
# pyright: reportDeprecated=false
# pyright: reportUnknownVariableType=false
# pyright: reportUnknownParameterType=false
# pyright: reportUnknownMemberType=false
# pyright: reportUnannotatedClassAttribute=false
# pyright: reportAny=false
# pyright: reportInvalidTypeForm=false
# pyright: reportCallIssue=false
# pyright: reportArgumentType=false
# pyright: reportUnknownArgumentType=false
from __future__ import annotations

import ctypes
from pathlib import Path

from polyplug import Runtime
from polyplug.loaders import (
    register_native_loader,
    register_dotnet_loader,
    register_python_loader,
    register_lua_loader,
    register_js_loader,
    register_js_deno_loader,
)

ABI_OK: int = 0
NULL_HANDLE: int = (1 << 64) - 1

TRANSFORMER_CONTRACT_ID: int = 0x3D53C682F3F5A9EF
REPORTER_CONTRACT_ID: int = 0x81D41D43E511D297

FNV_OFFSET: int = 0xCBF29CE484222325
FNV_PRIME: int = 0x00000100000001B3


class StringView(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]


class AbiError(ctypes.Structure):
    _fields_ = [
        ("code", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
        ("message", StringView),
    ]


class PluginVTable(ctypes.Structure):
    _fields_ = [
        ("contract_id", ctypes.c_uint64),
        ("contract_version", ctypes.c_uint32),
        ("function_count", ctypes.c_uint32),
        ("functions", ctypes.c_void_p),
    ]


ABI_FN_TYPE = ctypes.CFUNCTYPE(AbiError, ctypes.c_void_p, ctypes.c_void_p)


GUESTS: list[tuple[str, str, int, str]] = [
    ("rust/decoder", "rust_transformer", TRANSFORMER_CONTRACT_ID, "transform"),
    ("rust/reporter", "rust_reporter", REPORTER_CONTRACT_ID, "report"),
    ("cpp/transformer", "cpp_transformer", TRANSFORMER_CONTRACT_ID, "transform"),
    ("cpp/reporter", "cpp_reporter", REPORTER_CONTRACT_ID, "report"),
    ("csharp/encoder", "csharp_transformer", TRANSFORMER_CONTRACT_ID, "transform"),
    ("csharp/reporter", "csharp_reporter", REPORTER_CONTRACT_ID, "report"),
    ("python/decoder", "python_transformer", TRANSFORMER_CONTRACT_ID, "transform"),
    ("python/reporter", "python_reporter", REPORTER_CONTRACT_ID, "report"),
    ("lua/transformer", "lua_transformer", TRANSFORMER_CONTRACT_ID, "transform"),
    ("lua/reporter", "lua_reporter", REPORTER_CONTRACT_ID, "report"),
    (
        "js_quickjs/transformer",
        "js_quickjs_transformer",
        TRANSFORMER_CONTRACT_ID,
        "transform",
    ),
    ("js_quickjs/reporter", "js_quickjs_reporter", REPORTER_CONTRACT_ID, "report"),
    (
        "js_deno/transformer",
        "js_deno_transformer",
        TRANSFORMER_CONTRACT_ID,
        "transform",
    ),
    ("js_deno/reporter", "js_deno_reporter", REPORTER_CONTRACT_ID, "report"),
]


def fnv1a_64(data: bytes) -> int:
    value: int = FNV_OFFSET
    for byte in data:
        value ^= byte
        value = (value * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return value


def bundle_id(name: str) -> int:
    return fnv1a_64(name.encode("utf-8"))


def string_view_to_str(view: StringView) -> str:
    if view.ptr is None or view.len == 0:
        return ""
    raw: bytes = ctypes.string_at(view.ptr, view.len)
    return raw.decode("utf-8", errors="replace")


def call_fn(vtable_ptr: ctypes.c_void_p, args_ptr: int, out_ptr: int) -> AbiError:
    vtable: PluginVTable = ctypes.cast(
        vtable_ptr, ctypes.POINTER(PluginVTable)
    ).contents
    functions_ptr: ctypes.POINTER(ctypes.c_void_p) = ctypes.cast(
        vtable.functions, ctypes.POINTER(ctypes.c_void_p)
    )
    fn_ptr: ctypes.c_void_p = functions_ptr[0]
    func = ABI_FN_TYPE(fn_ptr)
    return func(ctypes.c_void_p(args_ptr), ctypes.c_void_p(out_ptr))


def main() -> None:
    repo_root: Path = Path(__file__).resolve().parents[3]
    guests_dir: Path = repo_root / "examples" / "guests"

    runtime = Runtime()

    register_native_loader(runtime)
    register_dotnet_loader(runtime)
    register_python_loader(runtime)
    register_lua_loader(runtime)
    register_js_loader(runtime)
    register_js_deno_loader(runtime)

    for guest_dir, _bundle_name, _contract_id, _fn_name in GUESTS:
        parts: list[str] = guest_dir.split("/")
        path: Path = guests_dir / parts[0] / parts[1]
        runtime.load_bundle(path)

    guards: list[object] = []
    for guest_dir, guest_bundle, contract_id, fn_name in GUESTS:
        packed: int = runtime.find_by_bundle(bundle_id(guest_bundle), contract_id, 0)
        if packed == NULL_HANDLE:
            raise RuntimeError(f"plugin not found: {guest_bundle}")

        guard = runtime.resolve_plugin(packed)
        guards.append(guard)
        vtable_ptr: ctypes.c_void_p = guard.get_vtable()

        input_bytes: bytes = b"hello"
        input_buf = ctypes.create_string_buffer(input_bytes)
        input_sv = StringView(
            ptr=ctypes.cast(input_buf, ctypes.c_void_p).value,
            len=len(input_bytes),
        )
        output_sv = StringView()

        err: AbiError = call_fn(
            vtable_ptr,
            ctypes.addressof(input_sv),
            ctypes.addressof(output_sv),
        )
        if err.code != ABI_OK:
            raise RuntimeError(f"call failed for {guest_dir}: code {err.code}")

        result: str = string_view_to_str(output_sv)
        label: str = f"[{guest_dir}]"
        print(f'{label:<30} {fn_name}("hello") = "{result}"')


if __name__ == "__main__":
    main()
