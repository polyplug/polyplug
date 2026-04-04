from __future__ import annotations

import ctypes
import enum
from typing import ClassVar

POLYPLUG_ABI_VERSION: int = 1
def fnv1a_64(data: &[u8]) -> ctypes.c_uint64:
    pass

def contract_id(name: &str, major: ctypes.c_uint32) -> ctypes.c_uint64:
    pass

def bundle_id(name: &str) -> ctypes.c_uint64:
    pass

def host_contract_id(name: &str, major: ctypes.c_uint32) -> ctypes.c_uint64:
    pass

def plugin_contract_id(name: &str, major: ctypes.c_uint32) -> ctypes.c_uint64:
    pass

