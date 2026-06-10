"""polyplug_abi — ABI types for the polyplug plugin runtime.

This package provides the frozen ABI types that match the Rust ABI exactly.
"""

from __future__ import annotations

from enum import IntEnum
from typing import Optional

from polyplug_abi.abi import (
    POLYPLUG_ABI_VERSION,
    AbiErrorCode,
    Compatibility,
    StringView,
    Buffer,
    Version,
    AbiError,
    GuestContractHandle,
    GuestContractInstance,
    HostContractInstance,
    HostContractInterface,
    VmLoaderData,
    NativeDispatch,
    VmDispatch,
    GuestContractInterface,
    HostApi,
    PluginDescriptor,
    BundleInitContext,
    Array,
    DependencyInfo,
    RuntimeConfig,
    DispatchType,
    DispatchMechanisms,
    ArenaOverflowBlock,
    CallArena,
    LogLevel,
    fnv1a_64,
    guest_contract_id,
    host_contract_id,
    bundle_id,
    FNV_OFFSET,
    FNV_PRIME,
)
from polyplug_abi.string_view_helper import (
    to_str,
    strip_prefix,
    starts_with,
    split,
    bytes_as_view,
)


class ReloadPhaseType(IntEnum):
    """Type tag for ReloadPhase variants."""

    Preparing = 0
    Reloaded = 1
    Failed = 2
    Unloading = 3


class ReloadPhase:
    """Python representation of hot-reload phase notification.

    Attributes:
        type: The phase type (Preparing, Reloaded, Failed, or Unloading).
        bundle_id: The FNV-1a hash of the bundle name.
        bundle_name: The human-readable bundle name.
        retry_count: Number of retry attempts (valid only for Preparing).
        reason: Failure reason string (valid only for Failed).
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
        """Return True if this is a Preparing phase."""
        return self.type == ReloadPhaseType.Preparing

    def is_reloaded(self) -> bool:
        """Return True if this is a Reloaded phase."""
        return self.type == ReloadPhaseType.Reloaded

    def is_failed(self) -> bool:
        """Return True if this is a Failed phase."""
        return self.type == ReloadPhaseType.Failed

    def is_unloading(self) -> bool:
        """Return True if this is an Unloading phase."""
        return self.type == ReloadPhaseType.Unloading

    def __repr__(self) -> str:
        return (
            f"ReloadPhase(type={self.type.name}, bundle_id={self.bundle_id}, "
            f"bundle_name={self.bundle_name!r}, retry_count={self.retry_count}, "
            f"reason={self.reason!r})"
        )


__all__ = [
    "POLYPLUG_ABI_VERSION",
    "AbiErrorCode",
    "Compatibility",
    "StringView",
    "Buffer",
    "Version",
    "AbiError",
    "GuestContractHandle",
    "GuestContractInstance",
    "HostContractInstance",
    "HostContractInterface",
    "VmLoaderData",
    "NativeDispatch",
    "VmDispatch",
    "GuestContractInterface",
    "HostApi",
    "PluginDescriptor",
    "BundleInitContext",
    "Array",
    "DependencyInfo",
    "RuntimeConfig",
    "DispatchType",
    "DispatchMechanisms",
    "LogLevel",
    "fnv1a_64",
    "guest_contract_id",
    "host_contract_id",
    "bundle_id",
    "FNV_OFFSET",
    "FNV_PRIME",
    "ReloadPhaseType",
    "ReloadPhase",
    "to_str",
    "strip_prefix",
    "starts_with",
    "split",
    "bytes_as_view",
]
