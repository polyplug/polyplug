from generated.guest.contracts import (
    REPORTERDataReporterPlugin,
    set_reporter_impl,
    polyplug_init,
)


class ReporterImpl(REPORTERDataReporterPlugin):
    def report(self, input: str) -> str:
        s = input.removeprefix("TRANSFORMED:")
        parts = s.split("|")
        if len(parts) >= 3:
            return f"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}"
        return "INVALID:format"


set_reporter_impl(ReporterImpl())
