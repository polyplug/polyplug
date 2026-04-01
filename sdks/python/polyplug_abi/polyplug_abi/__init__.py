"""polyplug_abi — ABI types for the polyplug plugin runtime.

This package provides the frozen ABI types that match the Rust ABI exactly.
"""

from __future__ import annotations

from enum import IntEnum
from typing import Optional

from polyplug_abi.abi import (
    POLYPLUG_ABI_VERSION,
    ABI_OK,
    ABI_ERROR_GENERIC,
    ABI_BUFFER_TOO_SMALL,
    ABI_ERROR_PANIC,
    ABI_ERROR_NOT_FOUND,
    ABI_ERROR_STALE_HANDLE,
    ABI_FUNCTION_NOT_AVAIL,
    StringView,
    Buffer,
    AbiError,
    PluginHandle,
    HostContext,
    NativeDispatch,
    VmDispatch,
    PluginInterface,
    HostVTable,
    PluginDescriptor,
    PluginContext,
    ExtensionEntry,
    RuntimeConfig,
    DispatchType,
    PluginDispatch,
    fnv1a_64,
    contract_id,
    extension_id,
    bundle_id,
    host_contract_id,
    plugin_contract_id,
    FNV_OFFSET,
    FNV_PRIME,
)
from polyplug_abi.string_view_helper import (
    to_str,
    strip_prefix,
    starts_with,
    split,
    str_as_view,
    str_as_view_owned,
)


class ReloadPhaseType(IntEnum):
    """Type tag for ReloadPhase variants."""

    PREPARING = 0
    RELOADED = 1
    FAILED = 2


class ReloadPhase:
    """Python representation of hot-reload phase notification.

    Attributes:
        type: The phase type (PREPARING, RELOADED, or FAILED).
        bundle_id: The FNV-1a hash of the bundle name.
        bundle_name: The human-readable bundle name.
        retry_count: Number of retry attempts (valid only for PREPARING).
        reason: Failure reason string (valid only for FAILED).
    """

    def __init__(
        self,
        type: ReloadPhaseType,
        bundle_id: int,
        bundle_name: str,
        retry_count: int = 0,
        reason: Optional[str] = None,
    ) -> None:
        self.type: ReloadPhaseType = type
        self.bundle_id: int = bundle_id
        self.bundle_name: str = bundle_name
        self.retry_count: int = retry_count
        self.reason: Optional[str] = reason

    def is_preparing(self) -> bool:
        """Return True if this is a PREPARING phase."""
        return self.type == ReloadPhaseType.PREPARING

    def is_reloaded(self) -> bool:
        """Return True if this is a RELOADED phase."""
        return self.type == ReloadPhaseType.RELOADED

    def is_failed(self) -> bool:
        """Return True if this is a FAILED phase."""
        return self.type == ReloadPhaseType.FAILED

    def __repr__(self) -> str:
        return (
            f"ReloadPhase(type={self.type.name}, bundle_id={self.bundle_id}, "
            f"bundle_name={self.bundle_name!r}, retry_count={self.retry_count}, "
            f"reason={self.reason!r})"
        )


__all__ = [
    "POLYPLUG_ABI_VERSION",
    "ABI_OK",
    "ABI_ERROR_GENERIC",
    "ABI_BUFFER_TOO_SMALL",
    "ABI_ERROR_PANIC",
    "ABI_ERROR_NOT_FOUND",
    "ABI_ERROR_STALE_HANDLE",
    "ABI_FUNCTION_NOT_AVAIL",
    "StringView",
    "Buffer",
    "AbiError",
    "PluginHandle",
    "HostContext",
    "NativeDispatch",
    "VmDispatch",
    "PluginInterface",
    "HostVTable",
    "PluginDescriptor",
    "PluginContext",
    "ExtensionEntry",
    "RuntimeConfig",
    "DispatchType",
    "PluginDispatch",
    "fnv1a_64",
    "contract_id",
    "extension_id",
    "bundle_id",
    "host_contract_id",
    "plugin_contract_id",
    "FNV_OFFSET",
    "FNV_PRIME",
    "ReloadPhaseType",
    "ReloadPhase",
    "to_str",
    "strip_prefix",
    "starts_with",
    "split",
    "str_as_view",
    "str_as_view_owned",
]
