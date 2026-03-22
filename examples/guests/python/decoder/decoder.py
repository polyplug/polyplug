from generated.guest.contracts import (
    DECODERPipelineDecoderPlugin,
    set_decoder_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import to_str, alloc_string


class DecoderImpl(DECODERPipelineDecoderPlugin):
    def decode(self, input):
        s = to_str(input).replace(",", "|")
        return alloc_string(f"DECODED:{s}")


set_decoder_impl(DecoderImpl())
