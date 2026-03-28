import { Contracts } from "./generated/guest/contracts.ts";
import { HostLoggerCaller } from "./generated/guest/host_contract_callers.ts";
import { getHostVtable, allocString, toString } from "../../../sdks/js/guest/polyplug_guest.ts";

export function doWork(input: string): string {
  const logger = HostLoggerCaller.fromHost(getHostVtable(), 1);

  if (logger && logger.isValid()) {
    logger.log(`Processing input: ${input}`);
    logger.log("Step 1: Analyzing input");
    logger.log("Step 2: Transforming data");
    logger.log("Step 3: Generating output");
  }

  return allocString(`WORKED: ${input.toUpperCase()}`);
}

Contracts.setWorkerImpl(doWork);

export { Contracts };