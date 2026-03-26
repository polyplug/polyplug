# tests/integration/python/test_hot_reload.py
# Unit tests for ReloadPhase and RuntimeConfig types.
#
# Run with: python -m pytest test_hot_reload.py -v
# Or: python test_hot_reload.py

import sys
import os
import unittest

# Add sdks/python/host to path
sys.path.insert(
    0,
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "sdks", "python", "host"),
)

from polyplug.abi import ReloadPhase, ReloadPhaseType
from polyplug.runtime_config import RuntimeConfig


class TestReloadPhaseTypeConstants(unittest.TestCase):
    """Tests for ReloadPhaseType enum constants."""

    def test_type_preparing_is_0(self):
        """TYPE_PREPARING should be 0."""
        self.assertEqual(0, ReloadPhaseType.PREPARING)

    def test_type_reloaded_is_1(self):
        """TYPE_RELOADED should be 1."""
        self.assertEqual(1, ReloadPhaseType.RELOADED)

    def test_type_failed_is_2(self):
        """TYPE_FAILED should be 2."""
        self.assertEqual(2, ReloadPhaseType.FAILED)


class TestReloadPhaseConstructor(unittest.TestCase):
    """Tests for ReloadPhase constructor."""

    def test_constructor_sets_all_properties(self):
        """Constructor should set all properties correctly."""
        phase = ReloadPhase(
            type=ReloadPhaseType.PREPARING,
            bundle_id=12345,
            bundle_name="TestBundle",
            retry_count=2,
            reason="Test reason",
        )
        self.assertEqual(ReloadPhaseType.PREPARING, phase.type)
        self.assertEqual(12345, phase.bundle_id)
        self.assertEqual("TestBundle", phase.bundle_name)
        self.assertEqual(2, phase.retry_count)
        self.assertEqual("Test reason", phase.reason)

    def test_constructor_uses_default_values(self):
        """Constructor should use default values for optional parameters."""
        phase = ReloadPhase(
            type=ReloadPhaseType.RELOADED, bundle_id=999, bundle_name="MyBundle"
        )
        self.assertEqual(ReloadPhaseType.RELOADED, phase.type)
        self.assertEqual(999, phase.bundle_id)
        self.assertEqual("MyBundle", phase.bundle_name)
        self.assertEqual(0, phase.retry_count)
        self.assertIsNone(phase.reason)

    def test_constructor_handles_none_reason(self):
        """Constructor should handle None reason."""
        phase = ReloadPhase(
            type=ReloadPhaseType.FAILED,
            bundle_id=1,
            bundle_name="Bundle",
            retry_count=0,
            reason=None,
        )
        self.assertIsNone(phase.reason)


class TestReloadPhaseHelperMethods(unittest.TestCase):
    """Tests for ReloadPhase helper methods."""

    def test_is_preparing_returns_true_for_preparing(self):
        """is_preparing should return True for PREPARING type."""
        phase = ReloadPhase(ReloadPhaseType.PREPARING, 1, "Bundle")
        self.assertTrue(phase.is_preparing())

    def test_is_preparing_returns_false_for_reloaded(self):
        """is_preparing should return False for RELOADED type."""
        phase = ReloadPhase(ReloadPhaseType.RELOADED, 1, "Bundle")
        self.assertFalse(phase.is_preparing())

    def test_is_reloaded_returns_true_for_reloaded(self):
        """is_reloaded should return True for RELOADED type."""
        phase = ReloadPhase(ReloadPhaseType.RELOADED, 1, "Bundle")
        self.assertTrue(phase.is_reloaded())

    def test_is_reloaded_returns_false_for_preparing(self):
        """is_reloaded should return False for PREPARING type."""
        phase = ReloadPhase(ReloadPhaseType.PREPARING, 1, "Bundle")
        self.assertFalse(phase.is_reloaded())

    def test_is_failed_returns_true_for_failed(self):
        """is_failed should return True for FAILED type."""
        phase = ReloadPhase(ReloadPhaseType.FAILED, 1, "Bundle", 0, "Error")
        self.assertTrue(phase.is_failed())

    def test_is_failed_returns_false_for_preparing(self):
        """is_failed should return False for PREPARING type."""
        phase = ReloadPhase(ReloadPhaseType.PREPARING, 1, "Bundle")
        self.assertFalse(phase.is_failed())


class TestRuntimeConfigDefaults(unittest.TestCase):
    """Tests for RuntimeConfig default values."""

    def test_default_max_retries_is_3(self):
        """Default hot_reload_max_retries should be 3."""
        config = RuntimeConfig()
        self.assertEqual(3, config.hot_reload_max_retries)

    def test_default_retry_interval_is_1000(self):
        """Default hot_reload_retry_interval_ms should be 1000."""
        config = RuntimeConfig()
        self.assertEqual(1000, config.hot_reload_retry_interval_ms)

    def test_default_abort_on_max_retries_is_true(self):
        """Default hot_reload_abort_on_max_retries should be True."""
        config = RuntimeConfig()
        self.assertTrue(config.hot_reload_abort_on_max_retries)


class TestRuntimeConfigCustomValues(unittest.TestCase):
    """Tests for RuntimeConfig custom values."""

    def test_custom_max_retries(self):
        """Should accept custom hot_reload_max_retries."""
        config = RuntimeConfig(hot_reload_max_retries=10)
        self.assertEqual(10, config.hot_reload_max_retries)

    def test_custom_retry_interval(self):
        """Should accept custom hot_reload_retry_interval_ms."""
        config = RuntimeConfig(hot_reload_retry_interval_ms=5000)
        self.assertEqual(5000, config.hot_reload_retry_interval_ms)

    def test_custom_abort_on_max_retries(self):
        """Should accept custom hot_reload_abort_on_max_retries."""
        config = RuntimeConfig(hot_reload_abort_on_max_retries=False)
        self.assertFalse(config.hot_reload_abort_on_max_retries)

    def test_partial_override_uses_defaults(self):
        """Partial override should use defaults for unspecified values."""
        config = RuntimeConfig(hot_reload_max_retries=5)
        self.assertEqual(5, config.hot_reload_max_retries)
        self.assertEqual(1000, config.hot_reload_retry_interval_ms)
        self.assertTrue(config.hot_reload_abort_on_max_retries)

    def test_zero_retries(self):
        """Should allow zero retries."""
        config = RuntimeConfig(hot_reload_max_retries=0)
        self.assertEqual(0, config.hot_reload_max_retries)

    def test_large_retry_interval(self):
        """Should allow large retry interval."""
        config = RuntimeConfig(hot_reload_retry_interval_ms=999999)
        self.assertEqual(999999, config.hot_reload_retry_interval_ms)


if __name__ == "__main__":
    unittest.main()
