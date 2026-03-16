# Python encoder plugin — implements pipeline.Encoder@1
# Input:  "name|value|42"
# Output: "ENCODED:name,value,42"

from generated.guest.contracts import (
    PYTHON_ENCODERPipelineEncoderPlugin,
    set_python_encoder_impl,
)
from polyplug_guest.abi import StringView

class EncoderPlugin(PYTHON_ENCODERPipelineEncoderPlugin):
    def encode(self, data: StringView) -> StringView:
        data_str = data.to_str()
        comma_sep = data_str.replace('|', ',')
        result = f"ENCODED:{comma_sep}"
        return StringView.from_string(result)

set_python_encoder_impl(EncoderPlugin())
