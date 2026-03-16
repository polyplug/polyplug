# Python reporter plugin — implements data.Reporter@1
# Input:  "name,value,42"
# Output: "REPORTED:name|value|42"

from generated.guest.contracts import (
    PYTHON_REPORTERDataReporterPlugin,
    set_python_reporter_impl,
)
from polyplug_guest.abi import StringView

class ReporterPlugin(PYTHON_REPORTERDataReporterPlugin):
    def report(self, data: StringView) -> StringView:
        data_str = data.to_str()
        pipe_sep = data_str.replace(',', '|')
        result = f"REPORTED:{pipe_sep}"
        return StringView.from_string(result)

set_python_reporter_impl(ReporterPlugin())
