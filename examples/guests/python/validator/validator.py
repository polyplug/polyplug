from generated.guest.contracts import (
    VALIDATORPipelineValidatorPlugin,
    set_validator_impl,
    polyplug_init,
)


class ValidatorImpl(VALIDATORPipelineValidatorPlugin):
    def validate(self, input: str) -> str:
        s = input.removeprefix("DECODED:")
        parts = s.split("|")
        if len(parts) >= 3 and parts[0] and parts[1]:
            try:
                int(parts[2])
                return f"VALID:{s}"
            except ValueError:
                pass
        return "INVALID:expected format is name|value|count"


set_validator_impl(ValidatorImpl())
