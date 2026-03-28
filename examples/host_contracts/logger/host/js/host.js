#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env

import { openPolyplug, runtimeNew, NULL_HANDLE } from "../../../sdks/js/host/polyplug.js";
import { ContractIds } from "./generated/host/callers.ts";
import { HostLoggerContractId } from "./generated/host/contracts.ts";

const pluginPath = Deno.env.get("POLYPLUG_PLUGIN_PATH")
  ?? "../../../examples/host_contracts/logger/plugins";

const libPath = Deno.env.get("POLYPLUG_LIB_PATH")
  ?? "/mnt/data/Projects/Utils/polyplug/target/release/deps/libpolyplug.so";

console.error(`loading plugins from: ${pluginPath}\n`);

const lib = openPolyplug(libPath);
const rt = runtimeNew(lib);

const logImpl = (message: string): void => {
  console.log(`[PLUGIN LOG] ${message}`);
};

rt.registerHostContract(HostLoggerContractId, logImpl);

const bundleNames = [];
for await (const entry of Deno.readDir(pluginPath)) {
  if (!entry.isDirectory) continue;
  const bundlePath = `${pluginPath}/${entry.name}`;
  try {
    rt.loadBundle(bundlePath);
    bundleNames.push(entry.name);
    console.error(`  loaded: ${entry.name}`);
  } catch (e) {
    console.error(`  failed to load ${entry.name}: ${e.message}`);
  }
}

if (bundleNames.length === 0) {
  console.error(`no plugins found in ${pluginPath}`);
  Deno.exit(1);
}

console.error(`\ndiscovered ${bundleNames.length} bundles\n`);

console.log("\n=== Logger Host (JavaScript/Deno) ===\n");

const inputStr = "hello world";
console.log(`Input: "${inputStr}"\n`);

const workerHandle = rt.findByContract(ContractIds.EXAMPLE_WORKER_CONTRACT_ID, 0);
if (workerHandle !== NULL_HANDLE) {
  const guard = rt.resolvePlugin(workerHandle);
  const result = guard.call(0, inputStr);
  console.log(`[host] do_work("${inputStr}") = "${result}"`);
}

console.log("\ndone.");