from generated.guest.contracts import (
    TRANSFORMERDataTransformerPlugin,
    set_transformer_impl,
    polyplug_init,
)


class TransformerImpl(TRANSFORMERDataTransformerPlugin):
    def transform(self, input: str) -> str:
        s = input.removeprefix("DECODED:")
        parts = s.split("|")
        if len(parts) >= 3:
            name = parts[0].upper()
            value = f"{parts[1]} (transformed)"
            count = int(parts[2]) + 1
            return f"TRANSFORMED:{name}|{value}|{count}"
        return "INVALID:format"


set_transformer_impl(TransformerImpl())
