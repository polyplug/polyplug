"""Runtime configuration options for hot-reload behavior and other settings."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class RuntimeConfig:
    """Configuration options for the Runtime.

    Attributes:
        hot_reload_enabled: Whether hot-reload is enabled for this runtime (default: False).
            Must be True to use reload_bundle() or file watcher.
        hot_reload_max_retries: Maximum retry attempts for hot-reload (default: 3).
            Set to 0 for infinite retries when hot_reload_abort_on_max_retries is False.
        hot_reload_retry_interval_ms: Interval between retry attempts in milliseconds (default: 1000).
        hot_reload_abort_on_max_retries: Whether to abort after max retries (default: True).
            If True: abort and fire Failed notification.
            If False: keep retrying forever.
    """

    hot_reload_enabled: bool = False
    hot_reload_max_retries: int = 3
    hot_reload_retry_interval_ms: int = 1000
    hot_reload_abort_on_max_retries: bool = True
