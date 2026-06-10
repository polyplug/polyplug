#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env
/**
 * @file host.js
 * @description Pipeline Host — Deno host demonstrating polyplug usage.
 *
 * Creates a runtime, registers the language loaders, scans the plugin
 * directory, loads every bundle it can, and prints the contracts each loaded
 * bundle provides (mirroring the C++ reference host's discovery output).
 */

import { openPolyplug, runtimeNew, bundleId, contractId } from "../../../sdks/js/host/mod.js";
import { registerNativeLoader } from "../../../sdks/js/loaders/native/mod.ts";
import { registerLuaLoader } from "../../../sdks/js/loaders/lua/mod.ts";
import { registerJsLoader } from "../../../sdks/js/loaders/js/mod.ts";
import { registerPythonLoader } from "../../../sdks/js/loaders/python/mod.ts";
import { registerDotnetLoader } from "../../../sdks/js/loaders/dotnet/mod.ts";
import {
  PipelineDecoderContract,
  DataTransformerContract,
  PipelineEncoderContract,
  DataReporterContract,
  PipelineValidatorContract,
} from "./generated/host/callers.ts";

const pluginPath = Deno.env.get("POLYPLUG_PLUGIN_PATH")
  ?? "../../../examples/plugins";

const libPath = Deno.env.get("POLYPLUG_LIB")
  ?? new URL("../../../target/release/deps/libpolyplug.so", import.meta.url).pathname;

console.error(`loading plugins from: ${pluginPath}\n`);

const lib = openPolyplug(libPath);
const rt = runtimeNew(lib);

// Register loaders for every runtime the example plugins may use. Loaders whose
// backing cdylib is unavailable are skipped so the host still runs for the rest.
const loaders = [
  { name: "native", register: () => registerNativeLoader(rt) },
  { name: "lua", register: () => registerLuaLoader(rt) },
  { name: "js-quickjs", register: () => registerJsLoader(rt) },
  { name: "python", register: () => registerPythonLoader(rt) },
  { name: "dotnet", register: () => registerDotnetLoader(rt) },
];
for (const loader of loaders) {
  try {
    loader.register();
  } catch (e) {
    console.error(`  loader ${loader.name} unavailable: ${e.message}`);
  }
}

/**
 * Parse a manifest.toml for its name and provided contracts.
 * @param {string} manifestPath - Path to manifest.toml.
 * @returns {{ name: string, provides: string[] }}
 */
function parseManifest(manifestPath) {
  const content = Deno.readTextFileSync(manifestPath);
  const nameMatch = content.match(/name\s*=\s*"([^"]+)"/);
  const name = nameMatch ? nameMatch[1] : "unknown";
  const provides = [];
  const listMatch = content.match(/provides\s*=\s*\[([^\]]*)\]/);
  if (listMatch) {
    for (const m of listMatch[1].matchAll(/"([^"]+)"/g)) {
      provides.push(m[1]);
    }
  }
  return { name, provides };
}

const bundleDirs = [];
for (const entry of Deno.readDirSync(pluginPath)) {
  if (entry.isDirectory) {
    bundleDirs.push(`${pluginPath}/${entry.name}`);
  }
}
bundleDirs.sort();

if (bundleDirs.length === 0) {
  console.error(`no plugins found in ${pluginPath}`);
  Deno.exit(1);
}

console.error(`discovered ${bundleDirs.length} bundles\n`);

const loaded = [];
for (const dir of bundleDirs) {
  const info = parseManifest(`${dir}/manifest.toml`);
  try {
    rt.loadBundle(dir);
    loaded.push(info);
    console.error(`  loaded: ${info.name}`);
  } catch (e) {
    console.error(`  skipped ${info.name}: ${e.message.split("\n")[0]}`);
  }
}

if (loaded.length === 0) {
  console.error("no bundles could be loaded");
  Deno.exit(1);
}

console.log("\n=== Pipeline Host (JavaScript/Deno) ===\n");

const inputStr = "name,value,42";
console.log(`Input: "${inputStr}"\n`);

const hex16 = (v) => v.toString(16).padStart(16, "0");

for (const bundle of loaded) {
  const bid = bundleId(bundle.name);
  for (const contract of bundle.provides) {
    const at = contract.lastIndexOf("@");
    if (at === -1) continue;
    const contractName = contract.slice(0, at);
    const major = Number(contract.slice(at + 1));
    const cid = contractId(contractName, major);
    console.log(
      `[${bundle.name}] provides ${contract} ` +
      `(bundle_id=0x${hex16(bid)}, contract_id=0x${hex16(cid)})`
    );
  }
}

console.log("");

// Dispatch the full data-processing pipeline through the generated host callers.
// Each stage resolves its contract via findGuestContract + resolveGuestContractInterface,
// then dispatches directly through the resolved interface. Stages whose contract is
// not registered are skipped (mirrors the rust/python reference hosts).
const decoder = PipelineDecoderContract.create(rt);
if (decoder) {
  const result = decoder.decode(inputStr);
  console.log(`[decoder] decode("${inputStr}") = "${result}"`);
  decoder.destroy();
}

const decoded = `DECODED:${inputStr.replace(/,/g, "|")}`;
const transformer = DataTransformerContract.create(rt);
if (transformer) {
  const result = transformer.transform(decoded);
  console.log(`[transformer] transform("${decoded}") = "${result}"`);
  transformer.destroy();
}

const transformed = "TRANSFORMED:NAME|value (transformed)|43";
const encoder = PipelineEncoderContract.create(rt);
if (encoder) {
  const result = encoder.encode(transformed);
  console.log(`[encoder] encode("${transformed}") = "${result}"`);
  encoder.destroy();
}

const reporter = DataReporterContract.create(rt);
if (reporter) {
  const result = reporter.report(transformed);
  console.log(`[reporter] report("${transformed}") = "${result}"`);
  reporter.destroy();
}

const validator = PipelineValidatorContract.create(rt);
if (validator) {
  const result = validator.validate(decoded);
  console.log(`[validator] validate("${decoded}") = "${result}"`);
  validator.destroy();
}

// Round-trip micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS): times the full
// host → runtime → native guest → return path (JS host calling the native decoder
// plugin and getting a string back). Point POLYPLUG_PLUGIN_PATH at native guests only.
const benchIters = Deno.env.get("POLYPLUG_BENCH_ITERS");
if (benchIters) {
  const n = parseInt(benchIters, 10);
  const benchDecoder = PipelineDecoderContract.create(rt);
  if (benchDecoder) {
    const warmup = Math.min(n, 10000);
    for (let i = 0; i < warmup; i++) benchDecoder.decode(inputStr);
    const t0 = performance.now();
    for (let i = 0; i < n; i++) benchDecoder.decode(inputStr);
    const t1 = performance.now();
    console.log(`ROUNDTRIP_NS=${(((t1 - t0) * 1e6) / n).toFixed(2)} LANG=js`);
    benchDecoder.destroy();
  }
}

console.log("\ndone.");
