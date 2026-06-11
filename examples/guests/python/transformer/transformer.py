from generated.guest.contracts import (
    TRANSFORMERDataTransformerPlugin,
    set_transformer_factory,
    polyplug_init,
)


class TransformerImpl(TRANSFORMERDataTransformerPlugin):
    """The factory receives the HostApi pointer at polyplug_init time."""

    def __init__(self, host_ptr: int) -> None:
        # Host handle for this runtime, captured at construction.
        self._host_ptr: int = host_ptr

    def transform(self, input: str) -> str:
        s = input.removeprefix("DECODED:")
        parts = s.split("|")
        if len(parts) >= 3:
            name = parts[0].upper()
            value = f"{parts[1]} (transformed)"
            count = int(parts[2]) + 1
            return f"TRANSFORMED:{name}|{value}|{count}"
        return "INVALID:format"


# Register the factory; the generated polyplug_init constructs the
# implementation with its owning runtime's host pointer.
set_transformer_factory(TransformerImpl)
