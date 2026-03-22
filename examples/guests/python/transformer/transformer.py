from generated.guest.contracts import (
    TRANSFORMERDataTransformerPlugin,
    set_transformer_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import to_str, alloc_string


class TransformerImpl(TRANSFORMERDataTransformerPlugin):
    def transform(self, input):
        s = to_str(input)
        if s.startswith("DECODED:"):
            s = s[8:]
        parts = s.split("|")
        if len(parts) >= 3:
            name = parts[0].upper()
            value = f"{parts[1]} (transformed)"
            count = int(parts[2]) + 1
            return alloc_string(f"TRANSFORMED:{name}|{value}|{count}")
        return alloc_string("INVALID:format")


set_transformer_impl(TransformerImpl())
