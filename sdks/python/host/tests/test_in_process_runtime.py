"""Focused unit tests for Python Runtime in-process resident ownership."""

from __future__ import annotations

import ctypes
import gc
import importlib.util
import sys
import weakref
from pathlib import Path

_TESTS_DIR = Path(__file__).resolve().parent
_PYTHON_SDK_DIR = _TESTS_DIR.parent.parent
_REPO_ROOT = _PYTHON_SDK_DIR.parent.parent
sys.path.insert(0, str(_PYTHON_SDK_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "polyplug_abi"))
sys.path.insert(0, str(_REPO_ROOT / "examples" / "hosts" / "python" / "generated"))

_RUNTIME_SPEC = importlib.util.spec_from_file_location(
    "polyplug_runtime_under_test", _PYTHON_SDK_DIR / "host" / "polyplug" / "runtime.py"
)
assert _RUNTIME_SPEC is not None and _RUNTIME_SPEC.loader is not None
_RUNTIME_MODULE = importlib.util.module_from_spec(_RUNTIME_SPEC)
sys.modules[_RUNTIME_SPEC.name] = _RUNTIME_MODULE
_RUNTIME_SPEC.loader.exec_module(_RUNTIME_MODULE)

AbiError = _RUNTIME_MODULE.AbiError
AbiErrorCode = _RUNTIME_MODULE.AbiErrorCode
InProcessBundleRegistration = _RUNTIME_MODULE.InProcessBundleRegistration
Runtime = _RUNTIME_MODULE.Runtime
from host.in_process import InProcessBundle, _DISPATCH  # noqa: E402
from polyplug_abi import StringView, to_str  # noqa: E402
from polyplug_abi.abi import GuestContractInstance, VmLoaderData  # noqa: E402


class _Bundle:
    def __init__(self, contract_count: int = 2) -> None:
        self.registration = InProcessBundleRegistration()
        self.registration.contract_count = contract_count
        self.prepare_calls = 0
        self.transferred = False

    def _reserve_transfer(self) -> None:
        if self.transferred:
            raise RuntimeError("in-process bundle has already been registered")
        self.transferred = True

    def _cancel_transfer(self) -> None:
        self.transferred = False

    def _in_process_registration(self) -> InProcessBundleRegistration:
        self.prepare_calls += 1
        return self.registration


def _runtime(register, unload) -> Runtime:
    runtime = Runtime.__new__(Runtime)
    runtime._host = 0xC0FFEE
    runtime._in_process_residents = {}
    runtime._register_in_process_bundle_fn = register
    runtime._unload_bundle_fn = unload
    runtime._get_error = lambda: "core rejected bundle"
    return runtime


def _decode(adapter, instance: GuestContractInstance, value: str) -> str:
    raw = value.encode("utf-8")
    raw_buffer = ctypes.create_string_buffer(raw)
    args = StringView(ctypes.cast(raw_buffer, ctypes.c_void_p), len(raw))
    output = StringView()
    error = AbiError()
    pointer = ctypes.cast(adapter._functions, ctypes.POINTER(ctypes.c_void_p))[0]
    dispatch = ctypes.cast(pointer, _DISPATCH)
    dispatch(
        adapter.context,
        instance,
        ctypes.byref(args),
        ctypes.byref(output),
        ctypes.byref(error),
    )
    assert error.code == AbiErrorCode.Ok
    return to_str(output)


def test_generated_adapters_are_stateful_and_runtime_local() -> None:
    class Counter:
        def __init__(self) -> None:
            self.calls = 0

        def decode(self, value: str) -> str:
            self.calls += 1
            return f"{value}:{self.calls}"

    first = InProcessBundle("python.first").add_pipeline_decoder(Counter)
    second = InProcessBundle("python.second").add_pipeline_decoder(Counter)
    first._in_process_registration()
    second._in_process_registration()
    first_adapter = first._adapters[0]
    second_adapter = second._adapters[0]
    first_instance = GuestContractInstance()
    second_instance = GuestContractInstance()
    first_adapter.interface.create_instance(
        first_adapter.context, VmLoaderData(), 0, None, ctypes.byref(first_instance)
    )
    second_adapter.interface.create_instance(
        second_adapter.context, VmLoaderData(), 0, None, ctypes.byref(second_instance)
    )

    assert _decode(first_adapter, first_instance, "first") == "first:1"
    assert _decode(first_adapter, first_instance, "first") == "first:2"
    assert _decode(second_adapter, second_instance, "second") == "second:1"

    first_adapter.interface.destroy_instance(
        first_adapter.context, VmLoaderData(), 0, first_instance
    )
    second_adapter.interface.destroy_instance(
        second_adapter.context, VmLoaderData(), 0, second_instance
    )

def _write_error(error_ptr: object, code: int) -> None:
    error = ctypes.cast(error_ptr, ctypes.POINTER(AbiError)).contents
    error.code = code


def test_registers_complete_multi_contract_bundle_once_and_roots_resident() -> None:
    registrations: list[int] = []

    def register(_host, registration_ptr, bundle_id_ptr, error_ptr) -> None:
        registration = ctypes.cast(
            registration_ptr, ctypes.POINTER(InProcessBundleRegistration)
        ).contents
        registrations.append(registration.contract_count)
        ctypes.cast(bundle_id_ptr, ctypes.POINTER(ctypes.c_uint64)).contents.value = 41
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(register, lambda *_: None)
    bundle = _Bundle()
    reference = weakref.ref(bundle)

    assert runtime.register_in_process_bundle(bundle) == 41
    del bundle
    gc.collect()

    assert registrations == [2]
    assert reference() is not None
    assert set(runtime._in_process_residents) == {41}


def test_failed_registration_never_installs_resident() -> None:
    def register(_host, _registration_ptr, _bundle_id_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.DuplicateProvider)

    runtime = _runtime(register, lambda *_: None)
    bundle = _Bundle()

    try:
        runtime.register_in_process_bundle(bundle)
    except RuntimeError as error:
        assert str(error) == "core rejected bundle"
    else:
        raise AssertionError("failed registration must raise")

    assert runtime._in_process_residents == {}
    assert bundle.transferred is False


def test_failed_unload_retains_resident_until_a_successful_logical_unload() -> None:
    def register(_host, _registration_ptr, bundle_id_ptr, error_ptr) -> None:
        ctypes.cast(bundle_id_ptr, ctypes.POINTER(ctypes.c_uint64)).contents.value = 42
        _write_error(error_ptr, AbiErrorCode.Ok)

    def failed_unload(_host, _bundle_id, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Generic)

    runtime = _runtime(register, failed_unload)
    bundle = _Bundle(contract_count=1)
    reference = weakref.ref(bundle)
    runtime.register_in_process_bundle(bundle)
    del bundle

    try:
        runtime.unload_bundle(42)
    except RuntimeError as error:
        assert str(error) == "core rejected bundle"
    else:
        raise AssertionError("failed unload must raise")

    gc.collect()
    assert reference() is not None
    assert 42 in runtime._in_process_residents

    def successful_unload(_host, _bundle_id, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime._unload_bundle_fn = successful_unload
    runtime.unload_bundle(42)
    gc.collect()

    assert reference() is None
    assert runtime._in_process_residents == {}
