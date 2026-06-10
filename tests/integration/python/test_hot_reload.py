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

from polyplug import ReloadPhase, ReloadPhaseType
from polyplug.runtime import RuntimeConfig


class TestReloadPhaseTypeConstants(unittest.TestCase):
    """Tests for ReloadPhaseType enum constants."""

    def test_type_preparing_is_0(self):
        """Preparing should be 0."""
        self.assertEqual(0, ReloadPhaseType.Preparing)

    def test_type_reloaded_is_1(self):
        """Reloaded should be 1."""
        self.assertEqual(1, ReloadPhaseType.Reloaded)

    def test_type_failed_is_2(self):
        """Failed should be 2."""
        self.assertEqual(2, ReloadPhaseType.Failed)

    def test_type_unloading_is_3(self):
        """Unloading should be 3."""
        self.assertEqual(3, ReloadPhaseType.Unloading)


class TestReloadPhaseConstructor(unittest.TestCase):
    """Tests for ReloadPhase constructor."""

    def test_constructor_sets_all_properties(self):
        """Constructor should set all properties correctly."""
        phase = ReloadPhase(
            type=ReloadPhaseType.Preparing,
            bundle_id=12345,
            bundle_name="TestBundle",
            reason="Test reason",
        )
        self.assertEqual(ReloadPhaseType.Preparing, phase.type)
        self.assertEqual(12345, phase.bundle_id)
        self.assertEqual("TestBundle", phase.bundle_name)
        self.assertEqual("Test reason", phase.reason)

    def test_constructor_uses_default_values(self):
        """Constructor should use default values for optional parameters."""
        phase = ReloadPhase(
            type=ReloadPhaseType.Reloaded, bundle_id=999, bundle_name="MyBundle"
        )
        self.assertEqual(ReloadPhaseType.Reloaded, phase.type)
        self.assertEqual(999, phase.bundle_id)
        self.assertEqual("MyBundle", phase.bundle_name)
        self.assertIsNone(phase.reason)

    def test_constructor_handles_none_reason(self):
        """Constructor should handle None reason."""
        phase = ReloadPhase(
            type=ReloadPhaseType.Failed,
            bundle_id=1,
            bundle_name="Bundle",
            reason=None,
        )
        self.assertIsNone(phase.reason)


class TestReloadPhaseHelperMethods(unittest.TestCase):
    """Tests for ReloadPhase helper methods."""

    def test_is_preparing_returns_true_for_preparing(self):
        """is_preparing should return True for Preparing type."""
        phase = ReloadPhase(ReloadPhaseType.Preparing, 1, "Bundle")
        self.assertTrue(phase.is_preparing())

    def test_is_preparing_returns_false_for_reloaded(self):
        """is_preparing should return False for Reloaded type."""
        phase = ReloadPhase(ReloadPhaseType.Reloaded, 1, "Bundle")
        self.assertFalse(phase.is_preparing())

    def test_is_reloaded_returns_true_for_reloaded(self):
        """is_reloaded should return True for Reloaded type."""
        phase = ReloadPhase(ReloadPhaseType.Reloaded, 1, "Bundle")
        self.assertTrue(phase.is_reloaded())

    def test_is_reloaded_returns_false_for_preparing(self):
        """is_reloaded should return False for Preparing type."""
        phase = ReloadPhase(ReloadPhaseType.Preparing, 1, "Bundle")
        self.assertFalse(phase.is_reloaded())

    def test_is_failed_returns_true_for_failed(self):
        """is_failed should return True for Failed type."""
        phase = ReloadPhase(ReloadPhaseType.Failed, 1, "Bundle", "Error")
        self.assertTrue(phase.is_failed())

    def test_is_failed_returns_false_for_preparing(self):
        """is_failed should return False for Preparing type."""
        phase = ReloadPhase(ReloadPhaseType.Preparing, 1, "Bundle")
        self.assertFalse(phase.is_failed())

    def test_is_unloading_returns_true_for_unloading(self):
        """is_unloading should return True for Unloading type."""
        phase = ReloadPhase(ReloadPhaseType.Unloading, 1, "Bundle")
        self.assertTrue(phase.is_unloading())

    def test_is_unloading_returns_false_for_preparing(self):
        """is_unloading should return False for Preparing type."""
        phase = ReloadPhase(ReloadPhaseType.Preparing, 1, "Bundle")
        self.assertFalse(phase.is_unloading())


class TestReloadPhaseTotality(unittest.TestCase):
    """The phase-type handling must be total: an unknown discriminant from a
    newer runtime must never raise (ctypes swallows exceptions inside the
    reload callback, silently killing it)."""

    def test_unknown_phase_type_passes_through(self):
        """An unknown raw discriminant is carried as-is, repr says Unknown."""
        phase = ReloadPhase(type=99, bundle_id=7, bundle_name="B")
        self.assertEqual(99, phase.type)
        self.assertIn("Unknown(99)", repr(phase))
        self.assertFalse(phase.is_preparing())
        self.assertFalse(phase.is_reloaded())
        self.assertFalse(phase.is_failed())
        self.assertFalse(phase.is_unloading())

    def test_no_retry_count_attribute(self):
        """The ABI has no retry-count field; the mirror must not fabricate one."""
        phase = ReloadPhase(ReloadPhaseType.Preparing, 1, "Bundle")
        self.assertFalse(hasattr(phase, "retry_count"))


class TestRuntimePerInstanceState(unittest.TestCase):
    """Rule 12: the Runtime class must hold NO class-level callback/config/
    keepalive statics — everything flows through __init__."""

    def test_no_class_level_statics(self):
        from polyplug.runtime import Runtime as RuntimeClass

        for attr in (
            "_on_reload_cb",
            "_config",
            "_c_callback",
            "_host_contract_impls",
            "_host_contract_callbacks",
            "_host_contract_interfaces",
        ):
            self.assertNotIn(
                attr,
                vars(RuntimeClass),
                f"Runtime.{attr} must be instance state, not a class static",
            )

    def test_no_class_level_registration_api(self):
        from polyplug.runtime import Runtime as RuntimeClass

        self.assertFalse(hasattr(RuntimeClass, "set_config"))
        self.assertIsNone(
            getattr(RuntimeClass, "on_reload", None),
            "class-level on_reload registration is gone (constructor arg)",
        )


class TestRuntimeConfigDefaults(unittest.TestCase):
    """Tests for RuntimeConfig default values (canonical 3-field ABI struct)."""

    def test_default_hot_reload_disabled(self):
        """Default hot_reload_enabled should be False."""
        config = RuntimeConfig()
        self.assertFalse(config.hot_reload_enabled)

    def test_enable_hot_reload(self):
        """hot_reload_enabled can be set to True."""
        config = RuntimeConfig()
        config.hot_reload_enabled = True
        self.assertTrue(config.hot_reload_enabled)



if __name__ == "__main__":
    unittest.main()
