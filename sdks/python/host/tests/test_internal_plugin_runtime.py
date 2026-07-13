"""Focused SDK coverage for generated internal-plugin provider registration."""

from __future__ import annotations

import ctypes
import gc
import importlib.util
import sys
import threading
import weakref
from pathlib import Path

_TESTS_DIR = Path(__file__).resolve().parent
_PYTHON_SDK_DIR = _TESTS_DIR.parent.parent
sys.path.insert(0, str(_PYTHON_SDK_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "polyplug_abi"))

_RUNTIME_SPEC = importlib.util.spec_from_file_location(
    "polyplug_runtime_under_test", _PYTHON_SDK_DIR / "host" / "polyplug" / "runtime.py"
)
assert _RUNTIME_SPEC is not None and _RUNTIME_SPEC.loader is not None
_RUNTIME_MODULE = importlib.util.module_from_spec(_RUNTIME_SPEC)
sys.modules[_RUNTIME_SPEC.name] = _RUNTIME_MODULE
_RUNTIME_SPEC.loader.exec_module(_RUNTIME_MODULE)

AbiError = _RUNTIME_MODULE.AbiError
AbiErrorCode = _RUNTIME_MODULE.AbiErrorCode
GuestContractHandle = _RUNTIME_MODULE.GuestContractHandle
Runtime = _RUNTIME_MODULE.Runtime
SupportedLanguage = _RUNTIME_MODULE.SupportedLanguage
from polyplug_abi.abi import GuestContractInterface, PluginDescriptor  # noqa: E402


_MANIFEST = (
    'name = "python.generated"\n'
    'id = 1\n'
    'version = "1.0.0"\n'
    'provides = ["pipeline.Decoder@1"]\n'
    'function_count = { "pipeline.Decoder@1" = 1 }\n'
    'needs_reinit_on_dep_reload = false\n'
)


class _InternalPlugin:
    def __init__(self, contract_count: int = 2) -> None:
        self._contracts = tuple(
            (PluginDescriptor(), GuestContractInterface()) for _ in range(contract_count)
        )
        self.transferred = False

    def _reserve_transfer(self) -> None:
        if self.transferred:
            raise RuntimeError("internal-plugin provider input has already been consumed")
        self.transferred = True

    def _internal_plugin_contracts(self):
        return self._contracts


class _Backend:
    def __init__(
        self,
        bundle_id: int = 41,
        begin_code: int = AbiErrorCode.Ok,
        commit_code: int = AbiErrorCode.Ok,
    ) -> None:
        self.bundle_id = bundle_id
        self.begin_code = begin_code
        self.commit_code = commit_code
        self.begins: list[tuple[int, bytes, int]] = []
        self.commits: list[int] = []
        self.aborts: list[int] = []
        self.destroy_result = True

    def begin_internal_plugin(self, host, manifest, language, bundle_id, error) -> None:
        self.begins.append((host, manifest, language))
        bundle_id.value = self.bundle_id
        error.code = self.begin_code

    def commit_internal_plugin_with_handles(
        self, _host, bundle_id, handles, handle_count, error
    ) -> None:
        self.commits.append(bundle_id)
        for index, handle in enumerate(handles):
            handle.index = 100 + index
            handle.generation = 7
        handle_count.value = len(handles)
        error.code = self.commit_code

    def abort_internal_plugin(self, _host, bundle_id) -> None:
        self.aborts.append(bundle_id)

    def destroy_host_interface(self, _host) -> bool:
        return self.destroy_result


class _PostPublicationFailureBackend(_Backend):
    def __init__(self, failure: str) -> None:
        super().__init__()
        self.failure = failure
        self.published_bundle_ids: set[int] = set()
        self.rooted_during_unload: list[bool] = []
        self.rooted_during_commit: list[bool] = []
        self.runtime: Runtime | None = None

    def commit_internal_plugin_with_handles(
        self, host, bundle_id, handles, handle_count, error
    ) -> None:
        assert self.runtime is not None
        self.rooted_during_commit.append(
            bundle_id in self.runtime._internal_plugin_residents
        )
        super().commit_internal_plugin_with_handles(host, bundle_id, handles, handle_count, error)
        self.published_bundle_ids.add(bundle_id)
        if self.failure == "interrupt":
            raise KeyboardInterrupt()
        if self.failure == "handle_count_mismatch":
            handle_count.value -= 1

    def native_unload(self, _host, bundle_id, error_ptr) -> None:
        assert self.runtime is not None
        self.rooted_during_unload.append(
            bundle_id in self.runtime._internal_plugin_residents
        )
        self.published_bundle_ids.remove(bundle_id)
        _write_error(error_ptr, AbiErrorCode.Ok)


class _PreNativeCommitInterruptionBackend(_Backend):
    def __init__(self) -> None:
        super().__init__()
        self.interrupt = True
        self.rooted_during_commit: list[bool] = []
        self.rooted_during_unload: list[bool] = []
        self.runtime: Runtime | None = None

    def commit_internal_plugin_with_handles(
        self, host, bundle_id, handles, handle_count, error
    ) -> None:
        assert self.runtime is not None
        self.rooted_during_commit.append(
            bundle_id in self.runtime._internal_plugin_residents
        )
        if self.interrupt:
            raise KeyboardInterrupt()
        super().commit_internal_plugin_with_handles(host, bundle_id, handles, handle_count, error)

    def native_unload(self, _host, bundle_id, error_ptr) -> None:
        assert self.runtime is not None
        self.rooted_during_unload.append(
            bundle_id in self.runtime._internal_plugin_residents
        )
        _write_error(error_ptr, AbiErrorCode.NotFound)


class _CommitPublicationBarrierBackend(_Backend):
    def __init__(self) -> None:
        super().__init__()
        self.runtime: Runtime | None = None
        self.commit_published = threading.Event()
        self.unload_attempted = threading.Event()
        self.native_unload_entered = threading.Event()
        self.allow_commit_return = threading.Event()
        self.allow_unload_return = threading.Event()
        self._unload_attempt_barrier = threading.Barrier(2)
        self.unload_errors: list[BaseException] = []
        self.unload_thread: threading.Thread | None = None

    def commit_internal_plugin_with_handles(
        self, host, bundle_id, handles, handle_count, error
    ) -> None:
        super().commit_internal_plugin_with_handles(host, bundle_id, handles, handle_count, error)
        self.commit_published.set()
        self.unload_thread = threading.Thread(target=self._attempt_unload)
        self.unload_thread.start()
        self._unload_attempt_barrier.wait(timeout=5)
        assert self.unload_attempted.wait(timeout=5)
        assert self.allow_commit_return.wait(timeout=5)

    def _attempt_unload(self) -> None:
        try:
            self._unload_attempt_barrier.wait(timeout=5)
            self.unload_attempted.set()
            assert self.runtime is not None
            self.runtime.unload_bundle(self.bundle_id)
        except BaseException as error:
            self.unload_errors.append(error)

    def native_unload(self, _host, _bundle_id, error_ptr) -> None:
        self.native_unload_entered.set()
        assert self.allow_unload_return.wait(timeout=5)
        _write_error(error_ptr, AbiErrorCode.Ok)

def _write_error(error_ptr: object, code: int) -> None:
    error = ctypes.cast(error_ptr, ctypes.POINTER(AbiError)).contents
    error.code = code


def _runtime(backend, register, unload) -> Runtime:
    runtime = Runtime.__new__(Runtime)
    runtime._host = 0xC0FFEE
    runtime._backend = backend
    runtime._internal_plugin_lock = threading.RLock()
    runtime._internal_plugin_residents = {}
    runtime._register_guest_contract_fn = register
    runtime._unload_bundle_fn = unload
    runtime._get_error = lambda: "core rejected plugin"
    return runtime


def test_generated_internal_plugin_stages_artifactless_manifest_and_exact_handles() -> None:
    registrations: list[tuple[object, object]] = []
    backend = _Backend()

    def register(_host, descriptor_ptr, interface_ptr, error_ptr) -> None:
        registrations.append((descriptor_ptr, interface_ptr))
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, lambda *_: None)
    plugin = _InternalPlugin()
    reference = weakref.ref(plugin)

    bundle_id, handles = runtime.register_generated_internal_plugin(_MANIFEST, plugin)
    del plugin
    gc.collect()

    assert bundle_id == 41
    assert [(handle.index, handle.generation) for handle in handles] == [(100, 7), (101, 7)]
    assert backend.begins == [(0xC0FFEE, _MANIFEST.encode(), int(SupportedLanguage.Python))]
    assert backend.commits == [41]
    assert backend.aborts == []
    assert len(registrations) == 2
    assert reference() is not None
    assert set(runtime._internal_plugin_residents) == {41}


def test_registration_failure_aborts_staged_plugin_and_consumes_input() -> None:
    backend = _Backend()

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.DuplicateProvider)

    runtime = _runtime(backend, register, lambda *_: None)
    plugin = _InternalPlugin(contract_count=1)

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, plugin)
    except RuntimeError as error:
        assert str(error) == "core rejected plugin"
    else:
        raise AssertionError("failed registration must raise")

    assert backend.commits == []
    assert backend.aborts == [41]
    assert runtime._internal_plugin_residents == {}
    assert plugin.transferred is True

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, plugin)
    except RuntimeError as error:
        assert str(error) == "internal-plugin provider input has already been consumed"
    else:
        raise AssertionError("failed registration must require fresh generated providers")


def test_commit_failure_aborts_pre_rooted_plugin_and_releases_it() -> None:
    backend = _Backend(commit_code=AbiErrorCode.Generic)

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, lambda *_: None)
    plugin = _InternalPlugin(contract_count=1)

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, plugin)
    except RuntimeError as error:
        assert str(error) == "core rejected plugin"
    else:
        raise AssertionError("failed commit must raise")

    assert backend.commits == [41]
    assert backend.aborts == [41]
    assert runtime._internal_plugin_residents == {}


def test_begin_failure_consumes_input_without_publishing_or_aborting() -> None:
    backend = _Backend(begin_code=AbiErrorCode.Generic)

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, lambda *_: None)
    plugin = _InternalPlugin(contract_count=1)

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, plugin)
    except RuntimeError as error:
        assert str(error) == "core rejected plugin"
    else:
        raise AssertionError("failed begin must raise")

    assert backend.commits == []
    assert backend.aborts == []
    assert runtime._internal_plugin_residents == {}
    assert plugin.transferred is True


def test_failed_unload_retains_internal_plugin_until_successful_unload() -> None:
    backend = _Backend(bundle_id=42)

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    def failed_unload(_host, _bundle_id, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Generic)

    runtime = _runtime(backend, register, failed_unload)
    plugin = _InternalPlugin(contract_count=1)
    reference = weakref.ref(plugin)
    runtime.register_generated_internal_plugin(_MANIFEST, plugin)
    del plugin

    try:
        runtime.unload_bundle(42)
    except RuntimeError as error:
        assert str(error) == "core rejected plugin"
    else:
        raise AssertionError("failed unload must raise")

    gc.collect()
    assert reference() is not None
    assert 42 in runtime._internal_plugin_residents

    def successful_unload(_host, _bundle_id, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime._unload_bundle_fn = successful_unload
    runtime.unload_bundle(42)
    gc.collect()

    assert reference() is None
    assert runtime._internal_plugin_residents == {}


def test_unload_waits_for_internal_registration_resident_publication() -> None:
    backend = _CommitPublicationBarrierBackend()

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, backend.native_unload)
    backend.runtime = runtime
    plugin = _InternalPlugin(contract_count=1)
    reference = weakref.ref(plugin)
    registration: list[tuple[int, tuple[GuestContractHandle, ...]]] = []
    registration_errors: list[BaseException] = []

    def register_plugin() -> None:
        try:
            registration.append(runtime.register_generated_internal_plugin(_MANIFEST, plugin))
        except BaseException as error:
            registration_errors.append(error)

    registration_thread = threading.Thread(target=register_plugin)
    registration_thread.start()

    assert backend.commit_published.wait(timeout=5)
    assert backend.unload_attempted.wait(timeout=5)
    assert not backend.native_unload_entered.is_set()

    backend.allow_commit_return.set()
    registration_thread.join(timeout=5)
    assert not registration_thread.is_alive()
    assert registration_errors == []
    assert [
        (bundle_id, [(handle.index, handle.generation) for handle in handles])
        for bundle_id, handles in registration
    ] == [(41, [(100, 7)])]
    assert runtime._internal_plugin_residents == {41: plugin}

    assert backend.native_unload_entered.wait(timeout=5)
    assert runtime._internal_plugin_residents == {41: plugin}

    del plugin
    backend.allow_unload_return.set()
    assert backend.unload_thread is not None
    backend.unload_thread.join(timeout=5)
    assert not backend.unload_thread.is_alive()
    assert backend.unload_errors == []
    gc.collect()
    assert reference() is None
    assert runtime._internal_plugin_residents == {}


def test_pre_native_commit_interrupt_aborts_staging_and_allows_fresh_retry() -> None:
    backend = _PreNativeCommitInterruptionBackend()

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, backend.native_unload)
    backend.runtime = runtime

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, _InternalPlugin(contract_count=1))
    except KeyboardInterrupt:
        pass
    else:
        raise AssertionError("pre-native interruption must be preserved")

    assert backend.commits == []
    assert backend.rooted_during_commit == [True]
    assert backend.rooted_during_unload == [True]
    assert backend.aborts == [41]
    assert runtime._internal_plugin_residents == {}

    backend.interrupt = False
    retry = _InternalPlugin(contract_count=1)
    assert runtime.register_generated_internal_plugin(_MANIFEST, retry)[0] == 41
    assert runtime._internal_plugin_residents == {41: retry}

    def successful_unload(_host, _bundle_id, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime._unload_bundle_fn = successful_unload
    runtime.unload_bundle(41)
    assert runtime._internal_plugin_residents == {}


def test_post_publication_keyboard_interrupt_keeps_rooted_until_unload_and_allows_retry() -> None:
    backend = _PostPublicationFailureBackend("interrupt")

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, backend.native_unload)
    backend.runtime = runtime
    interrupted = _InternalPlugin(contract_count=1)

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, interrupted)
    except KeyboardInterrupt:
        pass
    else:
        raise AssertionError("post-publication interruption must be preserved")

    assert backend.rooted_during_commit == [True]
    assert backend.rooted_during_unload == [True]
    assert backend.published_bundle_ids == set()
    assert runtime._internal_plugin_residents == {}
    assert backend.aborts == []

    backend.failure = ""
    retry = _InternalPlugin(contract_count=1)
    assert runtime.register_generated_internal_plugin(_MANIFEST, retry)[0] == 41
    assert runtime._internal_plugin_residents == {41: retry}
    runtime.unload_bundle(41)
    assert backend.published_bundle_ids == set()
    assert runtime._internal_plugin_residents == {}


def test_post_publication_interrupt_retains_root_when_unload_fails_then_allows_retry() -> None:
    backend = _PostPublicationFailureBackend("interrupt")

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, lambda *_: None)
    backend.runtime = runtime
    interrupted = _InternalPlugin(contract_count=1)
    reference = weakref.ref(interrupted)
    rooted_during_failed_unload: list[bool] = []

    def failed_unload(_host, bundle_id, error_ptr) -> None:
        rooted_during_failed_unload.append(
            bundle_id in runtime._internal_plugin_residents
        )
        _write_error(error_ptr, AbiErrorCode.Generic)

    runtime._unload_bundle_fn = failed_unload
    try:
        runtime.register_generated_internal_plugin(_MANIFEST, interrupted)
    except KeyboardInterrupt:
        pass
    else:
        raise AssertionError("post-publication interruption must be preserved")

    del interrupted
    gc.collect()
    assert backend.rooted_during_commit == [True]
    assert rooted_during_failed_unload == [True]
    assert backend.published_bundle_ids == {41}
    assert reference() is not None
    assert 41 in runtime._internal_plugin_residents
    assert backend.aborts == []

    runtime._unload_bundle_fn = backend.native_unload
    runtime.unload_bundle(41)
    gc.collect()
    assert backend.published_bundle_ids == set()
    assert reference() is None
    assert runtime._internal_plugin_residents == {}

    backend.failure = ""
    retry = _InternalPlugin(contract_count=1)
    assert runtime.register_generated_internal_plugin(_MANIFEST, retry)[0] == 41
    runtime.unload_bundle(41)
    assert backend.published_bundle_ids == set()
    assert runtime._internal_plugin_residents == {}


def test_post_publication_handle_count_mismatch_unloads_and_allows_retry() -> None:
    backend = _PostPublicationFailureBackend("handle_count_mismatch")

    def register(_host, _descriptor_ptr, _interface_ptr, error_ptr) -> None:
        _write_error(error_ptr, AbiErrorCode.Ok)

    runtime = _runtime(backend, register, backend.native_unload)
    backend.runtime = runtime
    mismatched = _InternalPlugin(contract_count=1)

    try:
        runtime.register_generated_internal_plugin(_MANIFEST, mismatched)
    except RuntimeError as error:
        assert str(error) == "committed handle count did not match generated providers"
    else:
        raise AssertionError("post-publication handle mismatch must raise")

    assert backend.rooted_during_commit == [True]
    assert backend.rooted_during_unload == [True]
    assert backend.published_bundle_ids == set()
    assert runtime._internal_plugin_residents == {}
    assert backend.aborts == []

    backend.failure = ""
    retry = _InternalPlugin(contract_count=1)
    assert runtime.register_generated_internal_plugin(_MANIFEST, retry)[0] == 41
    assert runtime._internal_plugin_residents == {41: retry}
    runtime.unload_bundle(41)
    assert backend.published_bundle_ids == set()
    assert runtime._internal_plugin_residents == {}


def test_runtime_finalizer_keeps_roots_until_destroy_succeeds() -> None:
    backend = _Backend()
    runtime = _runtime(backend, lambda *_: None, lambda *_: None)
    resident = object()
    runtime._internal_plugin_residents = {41: resident}

    backend.destroy_result = False
    Runtime.__del__(runtime)
    assert runtime._host == 0xC0FFEE
    assert runtime._internal_plugin_residents == {41: resident}

    backend.destroy_result = True
    Runtime.__del__(runtime)
    assert runtime._host == 0
    assert runtime._internal_plugin_residents == {}


def test_generated_internal_plugin_caller_uses_exact_committed_handle() -> None:
    runtime = _runtime(_Backend(), lambda *_: None, lambda *_: None)
    handle = GuestContractHandle(index=88, generation=9)

    class Caller:
        def __init__(self, received, host, owner) -> None:
            self.received = received
            self.host = host
            self.owner = owner

    caller = runtime.create_generated_internal_plugin_caller(Caller, handle)

    assert caller.received.index == 88
    assert caller.received.generation == 9
    assert caller.host.value == 0xC0FFEE
    assert caller.owner is runtime
