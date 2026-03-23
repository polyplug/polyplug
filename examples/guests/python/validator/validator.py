from generated.guest.contracts import (
    VALIDATORPipelineValidatorPlugin,
    set_validator_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import alloc_string
from polyplug_abi.helpers import strip_prefix, split


class ValidatorImpl(VALIDATORPipelineValidatorPlugin):
    def validate(self, input):
        s = strip_prefix(input, "DECODED:")
        parts = s.split("|")
        if len(parts) >= 3 and parts[0] and parts[1]:
            try:
                int(parts[2])
                return alloc_string(f"VALID:{s}")
            except ValueError:
                pass
        return alloc_string("INVALID:expected format is name|value|count")


set_validator_impl(ValidatorImpl())
