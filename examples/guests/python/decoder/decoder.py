# Python decoder plugin — implements pipeline.Decoder@1
# Input:  "name,value,42"
# Output: "DECODED:name|value|42"

from generated.guest.contracts import (
    PYTHON_DECODERPipelineDecoderPlugin,
    set_python_decoder_impl,
)
from polyplug_guest.abi import StringView

class DecoderPlugin(PYTHON_DECODERPipelineDecoderPlugin):
    def decode(self, input_sv: StringView) -> StringView:
        input_str = input_sv.to_str()
        parts = input_str.split(',')
        joined = '|'.join(parts)
        result = f"DECODED:{joined}"
        return StringView.from_string(result)

# Register implementation
set_python_decoder_impl(DecoderPlugin())
