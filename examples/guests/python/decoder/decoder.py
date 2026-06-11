from generated.guest.contracts import (
    DECODERPipelineDecoderPlugin,
    set_decoder_factory,
    polyplug_init,
)


class DecoderImpl(DECODERPipelineDecoderPlugin):
    """The factory receives the HostApi pointer at polyplug_init time."""

    def __init__(self, host_ptr: int) -> None:
        # Host handle for this runtime, captured at construction.
        self._host_ptr: int = host_ptr

    def decode(self, input: str) -> str:
        return f"DECODED:{input.replace(',', '|')}"


# Register the factory; the generated polyplug_init constructs the
# implementation with its owning runtime's host pointer.
set_decoder_factory(DecoderImpl)
