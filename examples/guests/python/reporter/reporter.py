from generated.guest.contracts import (
    REPORTERDataReporterPlugin,
    set_reporter_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import alloc_string
from polyplug_abi.helpers import strip_prefix


class ReporterImpl(REPORTERDataReporterPlugin):
    def report(self, input):
        s = strip_prefix(input, "TRANSFORMED:")
        parts = s.split("|")
        if len(parts) >= 3:
            return alloc_string(
                f"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}"
            )
        return alloc_string("INVALID:format")


set_reporter_impl(ReporterImpl())
