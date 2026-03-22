#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env
/**
 * @file host.js
 * @description Pipeline Host — Deno host demonstrating polyplug usage.
 */

import { openPolyplug, runtimeNew, NULL_HANDLE, onReload, setConfig } from "../../../host-libs/js-deno/polyplug.js";
import { RuntimeConfig } from "../../../host-libs/js-deno/polyplug/runtime_config.js";
import { ContractIds } from "./generated/host/callers.ts";

const pluginPath = Deno.env.get("POLYPLUG_PLUGIN_PATH")
  ?? "../../../examples/plugins";

const libPath = Deno.env.get("POLYPLUG_LIB_PATH")
  ?? "/mnt/data/Projects/Utils/polyplug/target/release/deps/libpolyplug.so";

console.error(`loading plugins from: ${pluginPath}\n`);

const _instances = new Map();

setConfig(new RuntimeConfig({
    hotReloadMaxRetries: 5,
    hotReloadRetryIntervalMs: 200,
    hotReloadAbortOnMaxRetries: false
}));

onReload((phase) => {
    if (phase.isPreparing()) {
        console.error(`[HOT-RELOAD] Preparing: ${phase.bundleName} (bundle_id=0x${phase.bundleId.toString(16).padStart(16, '0')}, retry ${phase.retryCount})`);
        if (_instances.has(phase.bundleId)) {
            _instances.delete(phase.bundleId);
            console.error(`[HOT-RELOAD] Cleared instances for bundle ${phase.bundleName}`);
        }
    } else if (phase.isReloaded()) {
        console.error(`[HOT-RELOAD] Reloaded: ${phase.bundleName} (bundle_id=0x${phase.bundleId.toString(16).padStart(16, '0')})`);
    } else if (phase.isFailed()) {
        console.error(`[HOT-RELOAD] Failed: ${phase.bundleName} (bundle_id=0x${phase.bundleId.toString(16).padStart(16, '0')}) - ${phase.reason}`);
    }
});

// Contract IDs are imported from generated code (polyplugc)

const lib = openPolyplug(libPath);
const rt = runtimeNew(lib);

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

console.log("\n=== Pipeline Host (JavaScript/Deno) ===\n");

const inputStr = "name,value,42";
console.log(`Input: "${inputStr}"\n`);

const decoderHandle = rt.findByContract(ContractIds.PIPELINE_DECODER_CONTRACT_ID, 0);
if (decoderHandle !== NULL_HANDLE) {
  const guard = rt.resolvePlugin(decoderHandle);
  const result = guard.call(0, inputStr);
  console.log(`[decoder] decode("${inputStr}") = "${result}"`);
}

const decoded = `DECODED:${inputStr.replace(/,/g, "|")}`;
const transformerHandle = rt.findByContract(ContractIds.DATA_TRANSFORMER_CONTRACT_ID, 0);
if (transformerHandle !== NULL_HANDLE) {
  const guard = rt.resolvePlugin(transformerHandle);
  const result = guard.call(0, decoded);
  console.log(`[transformer] transform("${decoded}") = "${result}"`);
}

const transformed = "TRANSFORMED:NAME|value (transformed)|43";
const encoderHandle = rt.findByContract(ContractIds.PIPELINE_ENCODER_CONTRACT_ID, 0);
if (encoderHandle !== NULL_HANDLE) {
  const guard = rt.resolvePlugin(encoderHandle);
  const result = guard.call(0, transformed);
  console.log(`[encoder] encode("${transformed}") = "${result}"`);
}

const reporterHandle = rt.findByContract(ContractIds.DATA_REPORTER_CONTRACT_ID, 0);
if (reporterHandle !== NULL_HANDLE) {
  const guard = rt.resolvePlugin(reporterHandle);
  const result = guard.call(0, transformed);
  console.log(`[reporter] report("${transformed}") = "${result}"`);
}

const validatorHandle = rt.findByContract(ContractIds.PIPELINE_VALIDATOR_CONTRACT_ID, 0);
if (validatorHandle !== NULL_HANDLE) {
  const guard = rt.resolvePlugin(validatorHandle);
  const result = guard.call(0, decoded);
  console.log(`[validator] validate("${decoded}") = "${result}"`);
}

console.log("\ndone.");