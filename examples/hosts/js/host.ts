#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env
/**
 * Pipeline Host — Deno host demonstrating polyplug usage.
 */

import { Runtime } from "https://deno.land/x/polyplug@0.1.0/mod.ts";
import { scanDir } from "https://deno.land/x/polyplug@0.1.0/scanner.ts";
import { toStr } from "https://deno.land/x/polyplug@0.1.0/helpers.ts";

const pluginPath = Deno.env.get("POLYPLUG_PLUGIN_PATH") ?? "examples/plugins";

console.error(`loading plugins from: ${pluginPath}\n`);

const rt = new Runtime({ pluginDir: pluginPath });
rt.registerNativeLoader();

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

for (const { manifest } of bundles) {
  const hasDecoder = manifest.provides.some((c: string) => c.startsWith("pipeline.Decoder"));

  if (hasDecoder) {
    const handle = rt.findByBundle(manifest.bundle_name, "pipeline.Decoder", 1);
    if (handle) {
      console.log(`[${manifest.bundle_name}] decoder ready`);
    }
  }
}

console.log("\ndone.");
