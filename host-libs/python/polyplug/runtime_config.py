# THIS FILE IS HAND-AUTHORED (part of polyplug host-libs/python)
"""Runtime configuration options for hot-reload behavior and other settings."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class RuntimeConfig:
    """Configuration options for the Runtime.

    This dataclass contains configurable parameters for hot-reload behavior
    and other runtime settings. It is designed to be extensible for future options.

    Attributes:
        hot_reload_max_retries: Maximum retry attempts for hot-reload (default: 3).
            Set to 0 for infinite retries when hot_reload_abort_on_max_retries is False.
        hot_reload_retry_interval_ms: Interval between retry attempts in milliseconds (default: 1000).
        hot_reload_abort_on_max_retries: Whether to abort after max retries (default: True).
            If True: abort and fire Failed notification.
            If False: keep retrying forever.
    """

    hot_reload_max_retries: int = 3
    hot_reload_retry_interval_ms: int = 1000
    hot_reload_abort_on_max_retries: bool = True
