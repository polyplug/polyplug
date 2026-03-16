#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env
/**
 * Pipeline Host — Deno host demonstrating polyplug usage.
 */

import { openPolyplug, runtimeNew } from "../../../host-libs/js/polyplug.ts";
import { scanDir } from "../../../host-libs/js/scanner.ts";
import { registerNativeLoader } from "../../../host-libs/js/loaders/native.ts";

const pluginPath = Deno.env.get("POLYPLUG_PLUGIN_PATH") ?? "examples/plugins";
const libPath = Deno.env.get("POLYPLUG_LIB_PATH") ?? "/mnt/data/Projects/Utils/polyplug/target/release/deps/libpolyplug.so";
const nativeLibPath = Deno.env.get("POLYPLUG_NATIVE_LIB_PATH") ?? "/mnt/data/Projects/Utils/polyplug/target/release/deps/libpolyplug_native.so";

console.error(`loading plugins from: ${pluginPath}\n`);

const lib = openPolyplug(libPath);
const rt = runtimeNew(lib);

// Register native loader
registerNativeLoader(lib, rt.ptr());

const bundles = scanDir(pluginPath);
if (bundles.length === 0) {
  console.error(`no plugins found in ${pluginPath}`);
  Deno.exit(1);
}

console.error(`discovered ${bundles.length} bundles\n`);

for (const { path, manifest } of bundles) {
  rt.loadBundle(path);
  console.error(`  loaded: ${manifest.bundle_name}`);
}

console.log("\n=== Pipeline Host (Deno) ===\n");

const input = "name,value,42";
console.log(`Input: "${input}"\n`);

for (const { manifest } of bundles) {
  const hasDecoder = manifest.provides.some((c: string) => c.startsWith("pipeline.Decoder@1"));
  const hasTransformer = manifest.provides.some((c: string) => c.startsWith("data.Transformer@1"));
  const hasEncoder = manifest.provides.some((c: string) => c.startsWith("pipeline.Encoder@1"));
  const hasReporter = manifest.provides.some((c: string) => c.startsWith("data.Reporter@1"));
  const hasValidator = manifest.provides.some((c: string) => c.startsWith("pipeline.Validator@1"));

  if (hasDecoder) {
    const handle = rt.findByBundle(manifest.bundle_name, "pipeline.Decoder", 1);
    if (handle) {
      const result = rt.call(handle, "decode", input);
      console.log(`[${manifest.bundle_name}] decode("${input}") = "${result}"`);
    }
  }

  if (hasTransformer) {
    const handle = rt.findByBundle(manifest.bundle_name, "data.Transformer", 1);
    if (handle) {
      const decoded = `DECODED:${input.replace(',', '|')}`;
      const result = rt.call(handle, "transform", decoded);
      console.log(`[${manifest.bundle_name}] transform("${decoded}") = "${result}"`);
    }
  }

  if (hasEncoder) {
    const handle = rt.findByBundle(manifest.bundle_name, "pipeline.Encoder", 1);
    if (handle) {
      const transformed = "TRANSFORMED:NAME|value (transformed)|43";
      const result = rt.call(handle, "encode", transformed);
      console.log(`[${manifest.bundle_name}] encode("${transformed}") = "${result}"`);
    }
  }

  if (hasReporter) {
    const handle = rt.findByBundle(manifest.bundle_name, "data.Reporter", 1);
    if (handle) {
      const transformed = "TRANSFORMED:NAME|value (transformed)|43";
      const result = rt.call(handle, "report", transformed);
      console.log(`[${manifest.bundle_name}] report("${transformed}") = "${result}"`);
    }
  }

  if (hasValidator) {
    const handle = rt.findByBundle(manifest.bundle_name, "pipeline.Validator", 1);
    if (handle) {
      const decoded = `DECODED:${input.replace(',', '|')}`;
      const result = rt.call(handle, "validate", decoded);
      console.log(`[${manifest.bundle_name}] validate("${decoded}") = "${result}"`);
    }
  }
}

console.log("\ndone.");
