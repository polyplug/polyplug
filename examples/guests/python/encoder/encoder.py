from generated.guest.contracts import (
    ENCODERPipelineEncoderPlugin,
    set_encoder_factory,
    polyplug_init,
)


class EncoderImpl(ENCODERPipelineEncoderPlugin):
    """The factory receives the HostApi pointer at polyplug_init time."""

    def __init__(self, host_ptr: int) -> None:
        # Host handle for this runtime, captured at construction.
        self._host_ptr: int = host_ptr

    def encode(self, input: str) -> str:
        s = input.removeprefix("TRANSFORMED:")
        return s.replace("|", ",")


# Register the factory; the generated polyplug_init constructs the
# implementation with its owning runtime's host pointer.
set_encoder_factory(EncoderImpl)
