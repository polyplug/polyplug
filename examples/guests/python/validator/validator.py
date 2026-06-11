from generated.guest.contracts import (
    VALIDATORPipelineValidatorPlugin,
    set_validator_factory,
    polyplug_init,
)


class ValidatorImpl(VALIDATORPipelineValidatorPlugin):
    """The factory receives the HostApi pointer at polyplug_init time."""

    def __init__(self, host_ptr: int) -> None:
        # Host handle for this runtime, captured at construction.
        self._host_ptr: int = host_ptr

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


# Register the factory; the generated polyplug_init constructs the
# implementation with its owning runtime's host pointer.
set_validator_factory(ValidatorImpl)
