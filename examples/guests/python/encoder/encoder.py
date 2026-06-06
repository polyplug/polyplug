from generated.guest.contracts import (
    ENCODERPipelineEncoderPlugin,
    set_encoder_impl,
    polyplug_init,
)


class EncoderImpl(ENCODERPipelineEncoderPlugin):
    def encode(self, input: str) -> str:
        s = input.removeprefix("TRANSFORMED:")
        return s.replace("|", ",")


set_encoder_impl(EncoderImpl())
