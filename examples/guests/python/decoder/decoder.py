from generated.guest.contracts import (
    DECODERPipelineDecoderPlugin,
    set_decoder_impl,
    polyplug_init,
)


class DecoderImpl(DECODERPipelineDecoderPlugin):
    def decode(self, input: str) -> str:
        return f"DECODED:{input.replace(',', '|')}"


set_decoder_impl(DecoderImpl())
