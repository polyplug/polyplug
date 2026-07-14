"""Focused Python host SDK coverage for metadata-introspection snapshots."""

from __future__ import annotations

import ctypes
import importlib.util
import sys
from types import SimpleNamespace
from pathlib import Path

_TESTS_DIR = Path(__file__).resolve().parent
_PYTHON_SDK_DIR = _TESTS_DIR.parent.parent
sys.path.insert(0, str(_PYTHON_SDK_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "polyplug_abi"))

_RUNTIME_SPEC = importlib.util.spec_from_file_location(
    "polyplug_runtime_introspection_under_test",
    _PYTHON_SDK_DIR / "host" / "polyplug" / "runtime.py",
)
assert _RUNTIME_SPEC is not None and _RUNTIME_SPEC.loader is not None
_RUNTIME_MODULE = importlib.util.module_from_spec(_RUNTIME_SPEC)
sys.modules[_RUNTIME_SPEC.name] = _RUNTIME_MODULE
_RUNTIME_SPEC.loader.exec_module(_RUNTIME_MODULE)

BundleDescriptorView = _RUNTIME_MODULE.BundleDescriptorView
GuestContractHandle = _RUNTIME_MODULE.GuestContractHandle
assert _RUNTIME_MODULE.HostApi is not None
RegisteredContractDescriptorView = _RUNTIME_MODULE.RegisteredContractDescriptorView
Runtime = _RUNTIME_MODULE.Runtime
assert _RUNTIME_MODULE.RuntimeIntrospection is not None
from polyplug_abi import BundleSourceKind  # noqa: E402
from polyplug_abi.abi import (  # noqa: E402
    Array,
    OwnedPluginDescriptorView,
    SupportedLanguage,
    Version,
)


class _IntrospectionFixture:
    def __init__(self, mode: str) -> None:
        self.mode = mode
        self.free_calls: list[tuple[int, int]] = []
        self._arrays: dict[int, ctypes.Array] = {}
        self.host = SimpleNamespace(
            reserved=mode != "older",
            list_bundles=self._list_bundle_ids,
        )
        self.table = SimpleNamespace(
            get_bundle_descriptor=self._get_bundle_descriptor,
            list_registered_guest_contracts=self._list_contract_handles,
            get_registered_contract_descriptor=self._get_contract_descriptor,
        )

        self.runtime = Runtime.__new__(Runtime)
        self.runtime._host = 0xC0FFEE
        self.runtime._host_struct = self.host
        self.runtime._free_fn = self._free_array
        self.runtime._introspection = lambda: self.table

    def _array(self, item_type, values: tuple[int | GuestContractHandle, ...], alignment: int) -> Array:
        count = len(values)
        if count:
            storage = (item_type * count)(*values)
        else:
            storage = (ctypes.c_uint8 * 1)()
        pointer = ctypes.addressof(storage)
        self._arrays[pointer] = storage
        return Array(pointer, count, alignment)

    def _list_bundle_ids(self, _host: int, out: int) -> None:
        values: tuple[int, ...] = (10, 20, 30, 40) if self.mode == "populated" else ()
        ctypes.cast(out, ctypes.POINTER(Array))[0] = self._array(
            ctypes.c_uint64, values, ctypes.alignment(ctypes.c_uint64))

    def _owned_bytes(self, value: bytes) -> Array:
        storage = ctypes.create_string_buffer(value)
        pointer = ctypes.addressof(storage)
        self._arrays[pointer] = storage
        return Array(pointer, len(value), ctypes.alignment(ctypes.c_uint8))

    @staticmethod
    def _version() -> Version:
        return Version(1, 2, 3)

    def _get_bundle_descriptor(self, _host: int, bundle_id: int, out: int) -> bool:
        if self.mode != "populated":
            return False
        descriptors = {
            10: (0, BundleSourceKind.Internal),
            20: (1, BundleSourceKind.Path),
            30: (2, BundleSourceKind.Code),
            40: (3, BundleSourceKind.Bytes),
        }
        if bundle_id not in descriptors:
            return False
        string_index, source_kind = descriptors[bundle_id]
        name = self._owned_bytes((b"internal", b"path", b"code", b"bytes")[string_index])
        descriptor = BundleDescriptorView(
            bundle_id,
            name.items,
            name.len,
            name.align,
            self._version(),
            int(SupportedLanguage.Python),
            int(source_kind),
        )
        ctypes.cast(out, ctypes.POINTER(BundleDescriptorView))[0] = descriptor
        return True

    def _list_contract_handles(self, _host: int, out: int) -> None:
        values: tuple[GuestContractHandle, ...] = (
            (GuestContractHandle * 2)(GuestContractHandle(1, 7), GuestContractHandle(2, 8))
            if self.mode == "populated"
            else ()
        )
        ctypes.cast(out, ctypes.POINTER(Array))[0] = self._array(
            GuestContractHandle,
            tuple(values),
            ctypes.alignment(GuestContractHandle),
        )

    def _get_contract_descriptor(self, _host: int, handle: GuestContractHandle, out: int) -> bool:
        if self.mode != "populated":
            return False
        descriptors = {
            1: (10, 101, b"alpha", b"example.alpha"),
            2: (20, 102, b"beta", b"example.beta"),
        }
        if handle.index not in descriptors:
            return False
        bundle_id, contract_id, plugin_name, contract_name = descriptors[handle.index]
        name = self._owned_bytes(plugin_name)
        owned_contract_name = self._owned_bytes(contract_name)
        descriptor = RegisteredContractDescriptorView(
            handle,
            bundle_id,
            contract_id,
            OwnedPluginDescriptorView(
                name.items,
                name.len,
                name.align,
                owned_contract_name.items,
                owned_contract_name.len,
                owned_contract_name.align,
                self._version(),
            ),
        )
        ctypes.cast(out, ctypes.POINTER(RegisteredContractDescriptorView))[0] = descriptor
        return True

    def _free_array(self, _host: int, pointer: int, size: int, alignment: int) -> None:
        self.free_calls.append((size, alignment))
        storage = self._arrays.pop(pointer, None)
        assert storage is not None, "SDK must free each native temporary array exactly once"
        ctypes.memset(pointer, 0xA5, max(size, 1))


class _FindAllOwnershipFixture:
    def __init__(self) -> None:
        self.free_calls: list[tuple[int, int]] = []
        self.resolve_calls: list[tuple[int, int]] = []
        self._arrays: dict[int, ctypes.Array] = {}
        self.runtime = Runtime.__new__(Runtime)
        self.runtime._host = 0xC0FFEE
        self.runtime._find_all_fn = self._find_all
        self.runtime._free_fn = self._free_array
        self.runtime._resolve_fn = self._resolve

    def _find_all(self, _host: int, _contract_id: int, _min_version: int, out: int) -> None:
        storage = (GuestContractHandle * 2)(
            GuestContractHandle(17, 23),
            GuestContractHandle(29, 31),
        )
        pointer = ctypes.addressof(storage)
        self._arrays[pointer] = storage
        ctypes.cast(out, ctypes.POINTER(Array))[0] = Array(
            pointer,
            len(storage),
            ctypes.alignment(GuestContractHandle),
        )

    def _free_array(self, _host: int, pointer: int, size: int, alignment: int) -> None:
        self.free_calls.append((size, alignment))
        storage = self._arrays.pop(pointer, None)
        assert storage is not None, "SDK must free the native result array exactly once"
        ctypes.memset(pointer, 0xA5, size)

    def _resolve(self, _host: int, handle: GuestContractHandle) -> int:
        self.resolve_calls.append((handle.index, handle.generation))
        return handle.index + handle.generation


def test_find_all_copies_handles_before_freeing_native_storage() -> None:
    fixture = _FindAllOwnershipFixture()

    handles = fixture.runtime.find_all_by_contract(0xA11CE, 1)

    assert fixture.free_calls == [
        (2 * ctypes.sizeof(GuestContractHandle), ctypes.alignment(GuestContractHandle))
    ]
    assert fixture._arrays == {}
    assert [(handle.index, handle.generation) for handle in handles] == [(17, 23), (29, 31)]
    assert fixture.runtime._resolve_fn(fixture.runtime._host, handles[0]) == 40
    assert fixture.runtime._resolve_fn(fixture.runtime._host, handles[1]) == 60
    assert fixture.resolve_calls == [(17, 23), (29, 31)]


def test_snapshot_descriptors_copy_all_source_kinds_and_contract_ownership() -> None:
    fixture = _IntrospectionFixture("populated")

    bundles = fixture.runtime.bundle_descriptors()
    assert [(bundle.id, bundle.name, bundle.source_kind) for bundle in bundles] == [
        (10, "internal", BundleSourceKind.Internal),
        (20, "path", BundleSourceKind.Path),
        (30, "code", BundleSourceKind.Code),
        (40, "bytes", BundleSourceKind.Bytes),
    ]
    assert all(isinstance(bundle.source_kind, BundleSourceKind) for bundle in bundles)
    assert not hasattr(bundles[2], "source")
    assert not hasattr(bundles[3], "bytes")

    contracts = fixture.runtime.registered_contract_descriptors()
    assert [
        (contract.handle.index, contract.bundle_id, contract.contract_id,
         contract.plugin_name, contract.contract_name)
        for contract in contracts
    ] == [
        (1, 10, 101, "alpha", "example.alpha"),
        (2, 20, 102, "beta", "example.beta"),
    ]
    assert fixture.free_calls == [
        (len(b"internal"), ctypes.alignment(ctypes.c_uint8)),
        (len(b"path"), ctypes.alignment(ctypes.c_uint8)),
        (len(b"code"), ctypes.alignment(ctypes.c_uint8)),
        (len(b"bytes"), ctypes.alignment(ctypes.c_uint8)),
        (4 * ctypes.sizeof(ctypes.c_uint64), ctypes.alignment(ctypes.c_uint64)),
        (len(b"alpha"), ctypes.alignment(ctypes.c_uint8)),
        (len(b"example.alpha"), ctypes.alignment(ctypes.c_uint8)),
        (len(b"beta"), ctypes.alignment(ctypes.c_uint8)),
        (len(b"example.beta"), ctypes.alignment(ctypes.c_uint8)),
        (2 * ctypes.sizeof(GuestContractHandle), ctypes.alignment(GuestContractHandle)),
    ]
    assert fixture._arrays == {}
    assert bundles[1].name == "path"
    assert contracts[1].contract_name == "example.beta"


def test_current_empty_introspection_frees_non_null_zero_length_arrays_once() -> None:
    fixture = _IntrospectionFixture("empty")

    assert fixture.runtime.bundle_descriptors() == []
    assert fixture.runtime.registered_contract_descriptors() == []
    assert fixture.free_calls == [
        (0, ctypes.alignment(ctypes.c_uint64)),
        (0, ctypes.alignment(GuestContractHandle)),
    ]
    assert fixture._arrays == {}


def test_legacy_runtime_without_introspection_returns_empty_snapshots() -> None:
    fixture = _IntrospectionFixture("older")

    assert fixture.runtime.bundle_descriptors() == []
    assert fixture.runtime.registered_contract_descriptors() == []
    assert fixture.free_calls == []
