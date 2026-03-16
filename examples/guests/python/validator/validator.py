# Python validator plugin — implements pipeline.Validator@1
# Input:  "name,value,42"
# Output: "VALID:name,value,42" or error

from generated.guest.contracts import (
    PYTHON_VALIDATORPipelineValidatorPlugin,
    set_python_validator_impl,
)
from polyplug_guest.abi import StringView, ABI_ERROR_GENERIC

class ValidatorPlugin(PYTHON_VALIDATORPipelineValidatorPlugin):
    def validate(self, data: StringView) -> StringView:
        data_str = data.to_str()
        parts = data_str.split(',')
        if len(parts) != 3:
            raise ValueError("invalid format: expected 3 fields")
        result = f"VALID:{data_str}"
        return StringView.from_string(result)

set_python_validator_impl(ValidatorPlugin())
