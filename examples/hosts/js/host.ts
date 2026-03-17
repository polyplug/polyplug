#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env
/**
 * Pipeline Host — Deno host demonstrating polyplug usage.
 */

import { openPolyplug, runtimeNew } from "../../../host-libs/js/polyplug.ts";

const pluginPath = Deno.env.get("POLYPLUG_PLUGIN_PATH") 
  ?? "../../../examples/plugins";

const libPath = Deno.env.get("POLYPLUG_LIB_PATH")
  ?? "/mnt/data/Projects/Utils/polyplug/target/release/deps/libpolyplug.so";

console.error(`loading plugins from: ${pluginPath}\n`);

const lib = openPolyplug(libPath);
const rt = runtimeNew(lib);

// Scan for plugins
const bundles = [];
for await (const entry of Deno.readDir(pluginPath)) {
  if (!entry.isDirectory) continue;
  const manifestPath = `${pluginPath}/${entry.name}/manifest.toml`;
  try {
    const content = await Deno.readTextFile(manifestPath);
    const nameMatch = content.match(/bundle_name\s*=\s*"([^"]+)"/);
    if (nameMatch) {
      bundles.push({ name: nameMatch[1], path: `${pluginPath}/${entry.name}` });
    }
  } catch {
    // No manifest
  }
}

if (bundles.length === 0) {
  console.error(`no plugins found in ${pluginPath}`);
  Deno.exit(1);
}

console.error(`discovered ${bundles.length} bundles\n`);

for (const bundle of bundles) {
  rt.loadBundle(bundle.path);
  console.error(`  loaded: ${bundle.name}`);
}

console.log("\n=== Pipeline Host (Deno) ===\n");
console.log("Deno host loaded all plugins successfully!");
console.log("\ndone.");
