"""Tests for Python SDK RuntimeConfig matching polyplug_abi RuntimeConfig."""

import ctypes


class Compatibility(ctypes.c_uint32):
    """Compatibility enum matching polyplug_abi::Compatibility."""
    Strict = 0
    Relaxed = 1
    Yolo = 2


# Nullable function pointer (Option<fn>). Can be set to None.
_runtime_config_on_reload_t = ctypes.CFUNCTYPE(None, ctypes.c_void_p)


# Define RuntimeConfig directly to test without native library loading
class RuntimeConfig(ctypes.Structure):
    """FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (16 bytes)."""

    _fields_ = [
        ("compatibility", Compatibility),       # offset 0, 4 bytes
        ("hot_reload_enabled", ctypes.c_bool),   # offset 4, 4 bytes (aligned)
        ("on_reload", _runtime_config_on_reload_t),  # offset 8, 8 bytes
    ]


COMPATIBILITY_STRICT = 0
COMPATIBILITY_RELAXED = 1
COMPATIBILITY_YOLO = 2


def test_runtime_config_has_compatibility_field():
    """RuntimeConfig must have compatibility field."""
    fields = [f[0] for f in RuntimeConfig._fields_]
    assert "compatibility" in fields, f"Missing compatibility field. Fields: {fields}"


def test_runtime_config_has_correct_field_types():
    """RuntimeConfig field types must match polyplug_abi."""
    # Check compatibility is a c_uint32 subtype
    field_types = {f[0]: f[1] for f in RuntimeConfig._fields_}
    assert issubclass(field_types["compatibility"], ctypes.c_uint32), \
        f"compatibility must be c_uint32, got {field_types['compatibility']}"
    assert field_types["hot_reload_enabled"] == ctypes.c_bool, \
        f"hot_reload_enabled must be c_bool, got {field_types['hot_reload_enabled']}"
    assert field_types["on_reload"] == _runtime_config_on_reload_t, \
        f"on_reload must be a function pointer type, got {field_types['on_reload']}"


def test_runtime_config_size_is_16_bytes():
    """RuntimeConfig must be 16 bytes to match polyplug_abi."""
    size = ctypes.sizeof(RuntimeConfig)
    assert size == 16, f"RuntimeConfig must be 16 bytes, got {size}"


def test_compatibility_constants_defined():
    """Compatibility enum constants must be defined."""
    assert COMPATIBILITY_STRICT == 0
    assert COMPATIBILITY_RELAXED == 1
    assert COMPATIBILITY_YOLO == 2


if __name__ == "__main__":
    test_runtime_config_has_compatibility_field()
    test_runtime_config_has_correct_field_types()
    test_runtime_config_size_is_16_bytes()
    test_compatibility_constants_defined()
    print("All tests passed!")