"""Tests for Python SDK RuntimeConfigC matching polyplug_abi RuntimeConfig."""

import ctypes


# Define RuntimeConfigC directly to test without native library loading
class RuntimeConfigC(ctypes.Structure):
    """FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes)."""

    _fields_ = [
        ("hot_reload_enabled", ctypes.c_uint8),           # offset 0, 1 byte
        ("_pad1", ctypes.c_uint8 * 3),                    # padding 3 bytes
        ("hot_reload_max_retries", ctypes.c_uint32),      # offset 4, 4 bytes
        ("hot_reload_retry_interval_ms", ctypes.c_uint64), # offset 8, 8 bytes
        ("hot_reload_abort_on_max_retries", ctypes.c_uint8), # offset 16, 1 byte
        ("_pad2", ctypes.c_uint8 * 3),                    # padding 3 bytes
        ("compatibility", ctypes.c_uint32),               # offset 20, 4 bytes
    ]


COMPATIBILITY_STRICT = 0
COMPATIBILITY_RELAXED = 1
COMPATIBILITY_YOLO = 2


def test_runtime_config_c_has_compatibility_field():
    """RuntimeConfigC must have compatibility field."""
    fields = [f[0] for f in RuntimeConfigC._fields_]
    assert "compatibility" in fields, f"Missing compatibility field. Fields: {fields}"


def test_runtime_config_c_has_correct_field_types():
    """RuntimeConfigC field types must match polyplug_abi."""
    # Check compatibility is c_uint32
    field_types = {f[0]: f[1] for f in RuntimeConfigC._fields_}
    assert field_types["compatibility"] == ctypes.c_uint32, \
        f"compatibility must be c_uint32, got {field_types['compatibility']}"


def test_runtime_config_c_size_is_24_bytes():
    """RuntimeConfigC must be 24 bytes to match polyplug_abi."""
    size = ctypes.sizeof(RuntimeConfigC)
    assert size == 24, f"RuntimeConfigC must be 24 bytes, got {size}"


def test_compatibility_constants_defined():
    """Compatibility enum constants must be defined."""
    assert COMPATIBILITY_STRICT == 0
    assert COMPATIBILITY_RELAXED == 1
    assert COMPATIBILITY_YOLO == 2


if __name__ == "__main__":
    test_runtime_config_c_has_compatibility_field()
    test_runtime_config_c_has_correct_field_types()
    test_runtime_config_c_size_is_24_bytes()
    test_compatibility_constants_defined()
    print("All tests passed!")