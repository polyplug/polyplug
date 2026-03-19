#!/usr/bin/env -S deno run --allow-read --allow-ffi --allow-env
/**
 * @file host.js
 * @description Pipeline Host — Deno host demonstrating polyplug usage.
 */

import { openPolyplug, runtimeNew, contractId, bundleId, onReload, setConfig, RuntimeConfig } from "../../../host-libs/js/polyplug.js";

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

const lib = openPolyplug(libPath);
const rt = runtimeNew(lib);

const bundles = [];
for await (const entry of Deno.readDir(pluginPath)) {
  if (!entry.isDirectory) continue;
  const manifestPath = `${pluginPath}/${entry.name}/manifest.toml`;
  try {
    const content = await Deno.readTextFile(manifestPath);
    const nameMatch = content.match(/bundle_name\s*=\s*"([^"]+)"/);
    const providesMatch = content.match(/provides\s*=\s*\[([^\]]+)\]/);
    if (nameMatch) {
      const provides = providesMatch
        ? providesMatch[1].match(/"([^"]+)"/g)?.map(s => s.slice(1, -1)) ?? []
        : [];
      bundles.push({ name: nameMatch[1], path: `${pluginPath}/${entry.name}`, provides });
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

console.log("\n=== Pipeline Host (JavaScript/Deno) ===\n");

const inputStr = "name,value,42";
console.log(`Input: "${inputStr}"\n`);

for (const bundle of bundles) {
  const bid = bundleId(bundle.name);
  
  for (const contract of bundle.provides) {
    const parts = contract.split('@');
    if (parts.length !== 2) continue;
    const contractName = parts[0];
    const versionParts = parts[1].split('.');
    const major = parseInt(versionParts[0]) || 1;
    
    const cid = contractId(contractName, major);
    const handle = rt.findByBundle(bid, cid, 0);
    
    if (handle === 0xFFFFFFFFFFFFFFFFn) continue;
    
    console.log(`[${bundle.name}] provides ${contract} (handle=${handle.toString(16)})`);
  }
}

console.log("\ndone.");
