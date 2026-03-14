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
import os
from pathlib import Path

from polyplug import Runtime
from polyplug.loaders import (
    register_native_loader,
    register_dotnet_loader,
    register_python_loader,
    register_lua_loader,
    register_js_loader,
)

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib  # type: ignore[no-redef]

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


def resolve_plugin_path() -> Path:
    env_path: str | None = os.environ.get("POLYPLUG_PLUGIN_PATH")
    if env_path:
        return Path(env_path)

    repo_root: Path = Path(__file__).resolve().parents[3]
    default_path: Path = repo_root / "examples" / "plugins"
    if default_path.is_dir():
        return default_path

    return Path("examples/plugins")


def scan_plugin_dir(plugin_dir: Path) -> list[dict]:
    bundles: list[dict] = []
    if not plugin_dir.is_dir():
        return bundles

    for entry in sorted(plugin_dir.iterdir()):
        if not entry.is_dir():
            continue
        manifest_path: Path = entry / "manifest.toml"
        if not manifest_path.exists():
            continue

        with open(manifest_path, "rb") as f:
            manifest: dict = tomllib.load(f)

        bundle_name: str = manifest.get("bundle_name", "")
        if not bundle_name:
            continue

        bundles.append(
            {
                "path": entry,
                "bundle_name": bundle_name,
                "provides": manifest.get("provides", []),
            }
        )

    bundles.sort(key=lambda b: b["bundle_name"])
    return bundles


def main() -> None:
    plugin_dir: Path = resolve_plugin_path()
    print(f"plugin directory: {plugin_dir}", file=__import__("sys").stderr)

    runtime = Runtime()

    register_native_loader(runtime)
    register_dotnet_loader(runtime)
    register_python_loader(runtime)
    register_lua_loader(runtime)
    register_js_loader(runtime)

    bundles: list[dict] = scan_plugin_dir(plugin_dir)
    if not bundles:
        raise RuntimeError(
            f"no plugins found in {plugin_dir}. Run examples/build_all.sh first."
        )

    print(f"discovered {len(bundles)} bundles", file=__import__("sys").stderr)

    for b in bundles:
        runtime.load_bundle(b["path"])
        print(f"  loaded: {b['bundle_name']}", file=__import__("sys").stderr)

    guards: list[object] = []
    for b in bundles:
        provides: list[str] = b["provides"]
        contract_id: int = 0
        fn_name: str = ""

        if "data.Transformer" in provides:
            contract_id = TRANSFORMER_CONTRACT_ID
            fn_name = "transform"
        elif "data.Reporter" in provides:
            contract_id = REPORTER_CONTRACT_ID
            fn_name = "report"
        else:
            continue

        packed: int = runtime.find_by_bundle(
            bundle_id(b["bundle_name"]), contract_id, 0
        )
        if packed == NULL_HANDLE:
            raise RuntimeError(f"plugin not found: {b['bundle_name']}")

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
            raise RuntimeError(f"call failed for {b['bundle_name']}: code {err.code}")

        result: str = string_view_to_str(output_sv)
        label: str = f"[{b['bundle_name']}]"
        print(f'{label:<30} {fn_name}("hello") = "{result}"')


if __name__ == "__main__":
    main()
