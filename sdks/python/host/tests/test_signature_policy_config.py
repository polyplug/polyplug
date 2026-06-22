"""Tests that the Python host SDK writes RuntimeConfig.signature_policy.

No native library is loaded: the ctypes RuntimeConfig struct is built from the
auto-generated ABI mirror and the signature_policy field is asserted directly.
This is the cheapest sufficient check that the setter surface targets the right
field at the right offset; full runtime-load coverage lives in the reload tests.
"""

import ctypes
import sys
from pathlib import Path

# Resolve the ABI mirror (sdks/python/abi) and the host package regardless of
# the invoking working directory.
_SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_SCRIPT_DIR.parent.parent / "abi"))

from abi import RuntimeConfig, SignaturePolicy


def test_signature_policy_field_at_offset_44():
    """signature_policy fills the former tail padding at offset 44."""
    assert RuntimeConfig.signature_policy.offset == 44


def test_runtime_config_is_48_bytes():
    """The struct stays 48 bytes after adding signature_policy."""
    assert ctypes.sizeof(RuntimeConfig) == 48


def test_signature_policy_defaults_to_off():
    """A zeroed RuntimeConfig has signature_policy == Off (0)."""
    config = RuntimeConfig()
    assert config.signature_policy == SignaturePolicy.Off.value
    assert config.signature_policy == 0


def test_setting_required_writes_value_2():
    """Setting Required writes the int 2 into the field."""
    config = RuntimeConfig()
    config.signature_policy = SignaturePolicy.Required.value
    assert config.signature_policy == SignaturePolicy.Required.value
    assert config.signature_policy == 2


def test_setting_warn_only_writes_value_1():
    """Setting WarnOnly writes the int 1 into the field."""
    config = RuntimeConfig()
    config.signature_policy = SignaturePolicy.WarnOnly.value
    assert config.signature_policy == 1


if __name__ == "__main__":
    test_signature_policy_field_at_offset_44()
    test_runtime_config_is_48_bytes()
    test_signature_policy_defaults_to_off()
    test_setting_required_writes_value_2()
    test_setting_warn_only_writes_value_1()
    print("All tests passed!")
