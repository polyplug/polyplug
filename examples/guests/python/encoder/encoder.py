from generated.guest.contracts import (
    ENCODERPipelineEncoderPlugin,
    set_encoder_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import to_str, alloc_string


class EncoderImpl(ENCODERPipelineEncoderPlugin):
    def encode(self, input):
        s = to_str(input)
        if s.startswith("TRANSFORMED:"):
            s = s[12:]
        return alloc_string(s.replace("|", ","))


set_encoder_impl(EncoderImpl())
