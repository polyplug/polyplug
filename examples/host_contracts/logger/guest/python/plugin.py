from generated.guest.contracts import (
    WORKERExampleWorkerPlugin,
    set_worker_impl,
    polyplug_abi_version,
    polyplug_init,
)
from generated.guest.host_contract_callers import HostLoggerCaller
from polyplug_guest import alloc_string
from polyplug_abi import get_host_vtable


class WorkerImpl(WORKERExampleWorkerPlugin):
    def do_work(self, input: str) -> str:
        logger = HostLoggerCaller.from_host(get_host_vtable(), 1)

        if logger and logger.is_valid():
            logger.log(f"Processing input: {input}")
            logger.log("Step 1: Analyzing input")
            logger.log("Step 2: Transforming data")
            logger.log("Step 3: Generating output")

        return alloc_string(f"WORKED: {input.upper()}")


set_worker_impl(WorkerImpl())
