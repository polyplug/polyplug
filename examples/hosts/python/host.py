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
from collections.abc import Iterable

from polyplug import Runtime

ABI_OK: int = 0
NULL_HANDLE: int = (1 << 64) - 1

DECODER_CONTRACT_ID: int = 0x133E62ABD6E7D5BE
TRANSFORMER_CONTRACT_ID: int = 0x0E3044133E12EB05
ENCODER_CONTRACT_ID: int = 0x12AD37F43386F752
REPORTER_CONTRACT_ID: int = 0xD50E539CAE219A15
VALIDATOR_CONTRACT_ID: int = 0x027ABCEBF8020D90

FNV_OFFSET: int = 0xCBF29CE484222325
FNV_PRIME: int = 0x00000100000001B3


class StringView(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
    ]


class Buffer(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_void_p),
        ("len", ctypes.c_size_t),
        ("cap", ctypes.c_size_t),
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


class DataRecord(ctypes.Structure):
    _fields_ = [
        ("name", StringView),
        ("value", StringView),
        ("count", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
    ]


class ValidationResult(ctypes.Structure):
    _fields_ = [
        ("valid", ctypes.c_uint8),
        ("_pad", ctypes.c_uint8 * 7),
        ("reason", StringView),
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


def abi_error_message(err: AbiError, fallback: str) -> str:
    if err.message.ptr is None or err.message.len == 0:
        return fallback
    return string_view_to_str(err.message)


def call_fn(
    vtable_ptr: ctypes.c_void_p, fn_id: int, args_ptr: int, out_ptr: int
) -> AbiError:
    vtable: PluginVTable = ctypes.cast(
        vtable_ptr, ctypes.POINTER(PluginVTable)
    ).contents
    functions_ptr: ctypes.POINTER(ctypes.c_void_p) = ctypes.cast(
        vtable.functions, ctypes.POINTER(ctypes.c_void_p)
    )
    fn_ptr: ctypes.c_void_p = functions_ptr[fn_id]
    func = ABI_FN_TYPE(fn_ptr)
    return func(ctypes.c_void_p(args_ptr), ctypes.c_void_p(out_ptr))


def ensure_handle(packed: int, label: str) -> int:
    if packed == NULL_HANDLE:
        raise RuntimeError(f"{label} plugin handle not found")
    return packed


def lookup_by_bundle(
    runtime: Runtime, bundle_name: str, contract_id: int, guards: list[object]
) -> ctypes.c_void_p:
    packed: int = ensure_handle(
        runtime.find_by_bundle(bundle_id(bundle_name), contract_id, 0),
        bundle_name,
    )
    guard = runtime.resolve_plugin(packed)
    guards.append(guard)
    return guard.get_vtable()


def load_bundles(runtime: Runtime, bundles: Iterable[Path]) -> None:
    bundle_list: list[Path] = list(bundles)
    print(f"Loading {len(bundle_list)} guest plugins...")
    for index, bundle in enumerate(bundle_list, start=1):
        runtime.load_bundle(bundle)
        parent_name: str = bundle.parent.name
        bundle_name: str = bundle.name
        print(f"  [OK]  {index:2d}/{len(bundle_list)} {parent_name}/{bundle_name}")


def run_pipeline(
    label: str,
    decoder_vt: ctypes.c_void_p,
    transformer_vt: ctypes.c_void_p,
    encoder_vt: ctypes.c_void_p,
    reporter_vt: ctypes.c_void_p,
    validator_vt: ctypes.c_void_p,
    input_csv: bytes,
) -> None:
    print(f"--- {label} ---")
    input_buf = ctypes.create_string_buffer(input_csv)
    buffer = Buffer(
        ptr=ctypes.cast(input_buf, ctypes.c_void_p).value,
        len=len(input_csv),
        cap=len(input_csv),
    )
    record = DataRecord()
    decode_err: AbiError = call_fn(
        decoder_vt,
        0,
        ctypes.addressof(buffer),
        ctypes.addressof(record),
    )
    if decode_err.code != ABI_OK:
        msg = abi_error_message(decode_err, "decode failed")
        raise RuntimeError(f"decode failed: {msg} (code {decode_err.code})")

    transformed = DataRecord()
    transform_err: AbiError = call_fn(
        transformer_vt,
        0,
        ctypes.addressof(record),
        ctypes.addressof(transformed),
    )
    if transform_err.code != ABI_OK:
        msg = abi_error_message(transform_err, "transform failed")
        raise RuntimeError(f"transform failed: {msg} (code {transform_err.code})")

    encoded = Buffer()
    encode_err: AbiError = call_fn(
        encoder_vt,
        0,
        ctypes.addressof(transformed),
        ctypes.addressof(encoded),
    )
    if encode_err.code != ABI_OK:
        msg = abi_error_message(encode_err, "encode failed")
        raise RuntimeError(f"encode failed: {msg} (code {encode_err.code})")
    output_bytes: bytes = (
        ctypes.string_at(encoded.ptr, encoded.len) if encoded.ptr else b""
    )
    print("Run output:", output_bytes.decode("utf-8", errors="replace").rstrip())

    report_sv = StringView()
    report_err: AbiError = call_fn(
        reporter_vt,
        0,
        ctypes.addressof(transformed),
        ctypes.addressof(report_sv),
    )
    if report_err.code != ABI_OK:
        msg = abi_error_message(report_err, "report failed")
        raise RuntimeError(f"report failed: {msg} (code {report_err.code})")
    report_str: str = string_view_to_str(report_sv)
    if report_str.strip():
        print("Run summary:", report_str)

    validation = ValidationResult()
    validate_err: AbiError = call_fn(
        validator_vt,
        0,
        ctypes.addressof(transformed),
        ctypes.addressof(validation),
    )
    if validate_err.code != ABI_OK:
        msg = abi_error_message(validate_err, "validate failed")
        raise RuntimeError(f"validate failed: {msg} (code {validate_err.code})")
    reason: str = string_view_to_str(validation.reason)
    status: str = "ok" if validation.valid else "invalid"
    print(f"Validation: {status} ({reason})")


def main() -> None:
    repo_root: Path = Path(__file__).resolve().parents[3]
    guests_dir: Path = repo_root / "examples" / "guests"

    bundles_main: list[Path] = [
        guests_dir / "rust" / "decoder",
        guests_dir / "rust" / "encoder",
        guests_dir / "cpp" / "transformer",
        guests_dir / "cpp" / "validator",
        guests_dir / "csharp" / "encoder",
        guests_dir / "csharp" / "reporter",
        guests_dir / "python" / "decoder",
        guests_dir / "python" / "reporter",
        guests_dir / "lua" / "transformer",
        guests_dir / "lua" / "validator",
        guests_dir / "js" / "validator",
        guests_dir / "js" / "reporter",
    ]

    print("=== polyplug C# host example ===")
    runtime_main = Runtime()
    load_bundles(runtime_main, bundles_main)

    guards: list[object] = []
    plugins: dict[str, ctypes.c_void_p] = {
        "decoder_rust": lookup_by_bundle(
            runtime_main, "csv_decoder", DECODER_CONTRACT_ID, guards
        ),
        "encoder_rust": lookup_by_bundle(
            runtime_main, "csv_encoder_rust", ENCODER_CONTRACT_ID, guards
        ),
        "transformer_cpp": lookup_by_bundle(
            runtime_main, "uppercase_transformer", TRANSFORMER_CONTRACT_ID, guards
        ),
        "validator_cpp": lookup_by_bundle(
            runtime_main, "cpp_validator", VALIDATOR_CONTRACT_ID, guards
        ),
        "encoder_csharp": lookup_by_bundle(
            runtime_main, "csv_encoder_csharp", ENCODER_CONTRACT_ID, guards
        ),
        "reporter_csharp": lookup_by_bundle(
            runtime_main, "csharp_reporter", REPORTER_CONTRACT_ID, guards
        ),
        "decoder_python": lookup_by_bundle(
            runtime_main, "python_decoder", DECODER_CONTRACT_ID, guards
        ),
        "reporter_python": lookup_by_bundle(
            runtime_main, "summary_reporter", REPORTER_CONTRACT_ID, guards
        ),
        "transformer_lua": lookup_by_bundle(
            runtime_main, "reverse_transformer", TRANSFORMER_CONTRACT_ID, guards
        ),
        "validator_lua": lookup_by_bundle(
            runtime_main, "lua_validator", VALIDATOR_CONTRACT_ID, guards
        ),
        "validator_js": lookup_by_bundle(
            runtime_main, "field_validator", VALIDATOR_CONTRACT_ID, guards
        ),
        "reporter_js": lookup_by_bundle(
            runtime_main, "js_reporter", REPORTER_CONTRACT_ID, guards
        ),
    }

    run_pipeline(
        "Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator",
        plugins["decoder_rust"],
        plugins["transformer_cpp"],
        plugins["encoder_rust"],
        plugins["reporter_csharp"],
        plugins["validator_cpp"],
        b"Alice,hello,3\n",
    )

    run_pipeline(
        "Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator",
        plugins["decoder_python"],
        plugins["transformer_lua"],
        plugins["encoder_csharp"],
        plugins["reporter_python"],
        plugins["validator_lua"],
        b"Bob,world,4\n",
    )

    run_pipeline(
        "Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator",
        plugins["decoder_rust"],
        plugins["transformer_cpp"],
        plugins["encoder_csharp"],
        plugins["reporter_js"],
        plugins["validator_js"],
        b"Cara,polyplug,5\n",
    )

    print("pipeline complete")


if __name__ == "__main__":
    main()
