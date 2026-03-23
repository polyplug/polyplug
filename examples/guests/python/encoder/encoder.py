from generated.guest.contracts import (
    ENCODERPipelineEncoderPlugin,
    set_encoder_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import alloc_string
from polyplug_abi.helpers import strip_prefix


class EncoderImpl(ENCODERPipelineEncoderPlugin):
    def encode(self, input):
        s = strip_prefix(input, "TRANSFORMED:")
        return alloc_string(s.replace("|", ","))


set_encoder_impl(EncoderImpl())
