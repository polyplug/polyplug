from generated.guest.contracts import (
    VALIDATORPipelineValidatorPlugin,
    set_validator_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import to_str, alloc_string


class ValidatorImpl(VALIDATORPipelineValidatorPlugin):
    def validate(self, input):
        s = to_str(input)
        if s.startswith("DECODED:"):
            s = s[8:]
        parts = s.split("|")
        if len(parts) >= 3 and parts[0] and parts[1]:
            try:
                int(parts[2])
                return alloc_string(f"VALID:{s}")
            except ValueError:
                pass
        return alloc_string("INVALID:expected format is name|value|count")


set_validator_impl(ValidatorImpl())
