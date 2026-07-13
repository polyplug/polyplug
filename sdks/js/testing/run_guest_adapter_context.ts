import "../host/tests/guest_adapter_context_test.ts";
import { runRegisteredTests } from "./harness.ts";

const result = await runRegisteredTests();
const host = globalThis as { process?: { exit: (code: number) => never } };
if (typeof Deno !== "undefined") {
    Deno.exit(result.failed === 0 ? 0 : 1);
}
host.process?.exit(result.failed === 0 ? 0 : 1);
