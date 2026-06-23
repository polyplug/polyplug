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
    Ed25519PublicKey,
    RuntimeConfig,
    SignaturePolicy,
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
    ends_with,
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

    Mirrors the ABI ``ReloadPhase`` struct exactly — there is no retry-count
    field in the ABI.

    Attributes:
        type: The phase type (Preparing, Reloaded, Failed, or Unloading), or
            the raw ``int`` discriminant when the runtime reports a phase this
            SDK version does not know (the conversion is total — see
            ``polyplug.runtime``).
        bundle_id: The FNV-1a hash of the bundle name.
        bundle_name: The human-readable bundle name.
        reason: Failure reason string (valid only for Failed).
    """

    def __init__(
        self,
        type: ReloadPhaseType | int,
        bundle_id: int,
        bundle_name: str,
        reason: Optional[str] = None,
    ) -> None:
        self.type: ReloadPhaseType | int = type
        self.bundle_id: int = bundle_id
        self.bundle_name: str = bundle_name
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
        type_name: str = (
            self.type.name
            if isinstance(self.type, ReloadPhaseType)
            else f"Unknown({self.type})"
        )
        return (
            f"ReloadPhase(type={type_name}, bundle_id={self.bundle_id}, "
            f"bundle_name={self.bundle_name!r}, reason={self.reason!r})"
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
    "Ed25519PublicKey",
    "RuntimeConfig",
    "SignaturePolicy",
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
    "ends_with",
    "split",
    "bytes_as_view",
]
