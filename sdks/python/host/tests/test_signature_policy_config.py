"""Tests that the Python host SDK marshals RuntimeConfig signing fields.

The signature_policy / trusted_keys assertions inspect the ctypes RuntimeConfig
struct directly from the auto-generated ABI mirror — the cheapest sufficient
check that each setter surface targets the right field at the right offset;
full runtime-load coverage lives in the reload tests. The trusted-keys array is
built with the same Runtime helper the constructor uses, so no native library
is exercised even though importing the host package loads it.
"""

import ctypes
import sys
from pathlib import Path

# Resolve the ABI mirror (sdks/python/abi as the `abi` namespace package), the
# host package, and the `polyplug_abi` shim regardless of the invoking cwd.
_SCRIPT_DIR = Path(__file__).resolve().parent
_HOST_DIR = _SCRIPT_DIR.parent
_PYTHON_SDK_DIR = _HOST_DIR.parent
sys.path.insert(0, str(_PYTHON_SDK_DIR))
sys.path.insert(0, str(_HOST_DIR))
sys.path.insert(0, str(_PYTHON_SDK_DIR / "polyplug_abi"))

from abi.abi import Ed25519PublicKey, RuntimeConfig, SignaturePolicy
from polyplug.runtime import Runtime


def test_signature_policy_field_at_offset_44():
    """signature_policy fills the former tail padding at offset 44."""
    assert RuntimeConfig.signature_policy.offset == 44


def test_runtime_config_is_72_bytes():
    """The struct is 72 bytes after adding signature_policy and trusted_keys."""
    assert ctypes.sizeof(RuntimeConfig) == 72


def test_trusted_keys_field_at_offset_48():
    """trusted_keys (the key-pinning Array) follows signature_policy at offset 48."""
    assert RuntimeConfig.trusted_keys.offset == 48


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


def test_trusted_keys_marshals_pointer_len_and_align():
    """A two-key allowlist fills the trusted-keys ptr/len/align config fields.

    No native library is loaded: the key buffer is built with the same helper
    the Runtime uses and the three flattened config fields are populated exactly
    as ``_create_runtime_with_options`` does, then asserted directly.
    """
    k1 = bytes(range(32))
    k2 = bytes(range(32, 64))

    buf = Runtime._build_trusted_keys([k1, k2])
    assert buf is not None

    config = RuntimeConfig()
    config.trusted_keys = ctypes.cast(buf, ctypes.c_void_p)
    config.trusted_keys_len = len(buf)
    config.trusted_keys__align = ctypes.alignment(Ed25519PublicKey)

    assert config.trusted_keys is not None
    assert config.trusted_keys != 0
    assert config.trusted_keys_len == 2
    assert config.trusted_keys__align == ctypes.alignment(Ed25519PublicKey)
    # The pointer addresses the contiguous owning array.
    assert config.trusted_keys == ctypes.addressof(buf)
    assert bytes(buf[0].bytes) == k1
    assert bytes(buf[1].bytes) == k2


def test_empty_trusted_keys_selects_tofu():
    """None and an empty iterable both yield no buffer (zeroed fields = TOFU)."""
    assert Runtime._build_trusted_keys(None) is None
    assert Runtime._build_trusted_keys([]) is None


def test_trusted_key_wrong_length_rejected():
    """A non-32-byte key is rejected before it can reach the runtime."""
    try:
        Runtime._build_trusted_keys([bytes(31)])
    except ValueError:
        pass
    else:
        raise AssertionError("expected ValueError for a 31-byte key")


if __name__ == "__main__":
    test_signature_policy_field_at_offset_44()
    test_runtime_config_is_72_bytes()
    test_trusted_keys_field_at_offset_48()
    test_signature_policy_defaults_to_off()
    test_setting_required_writes_value_2()
    test_setting_warn_only_writes_value_1()
    test_trusted_keys_marshals_pointer_len_and_align()
    test_empty_trusted_keys_selects_tofu()
    test_trusted_key_wrong_length_rejected()
    print("All tests passed!")
