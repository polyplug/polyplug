from generated.guest.contracts import (
    REPORTERDataReporterPlugin,
    set_reporter_factory,
    polyplug_init,
)


class ReporterImpl(REPORTERDataReporterPlugin):
    """The factory receives the HostApi pointer at polyplug_init time."""

    def __init__(self, host_ptr: int) -> None:
        # Host handle for this runtime, captured at construction.
        self._host_ptr: int = host_ptr

    def report(self, input: str) -> str:
        s = input.removeprefix("TRANSFORMED:")
        parts = s.split("|")
        if len(parts) >= 3:
            return f"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}"
        return "INVALID:format"


# Register the factory; the generated polyplug_init constructs the
# implementation with its owning runtime's host pointer.
set_reporter_factory(ReporterImpl)
