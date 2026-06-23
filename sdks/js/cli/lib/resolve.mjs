/**
 * Platform resolution for @polyplug/cli.
 *
 * Maps (process.platform, process.arch) to the optional platform package name
 * and the binary filename that CI injects into it.
 *
 * This module has zero I/O — it is pure logic so it can be unit-tested without
 * a real binary present.
 */

/** @typedef {{ packageName: string, binaryName: string }} PlatformInfo */

/**
 * Supported platform → package mappings.
 * Key format: "<platform>/<arch>" matching Node.js process.platform / process.arch.
 *
 * @type {Map<string, PlatformInfo>}
 */
const PLATFORM_MAP = new Map([
  ["linux/x64",   { packageName: "@polyplug/cli-linux-x64",   binaryName: "polyplugc"     }],
  ["darwin/arm64",{ packageName: "@polyplug/cli-darwin-arm64", binaryName: "polyplugc"     }],
  ["win32/x64",   { packageName: "@polyplug/cli-win32-x64",   binaryName: "polyplugc.exe" }],
]);

/**
 * Resolve the platform info for the given platform/arch pair.
 *
 * @param {string} platform - Value of process.platform (e.g. "linux", "darwin", "win32").
 * @param {string} arch     - Value of process.arch (e.g. "x64", "arm64").
 * @returns {PlatformInfo} The resolved package name and binary filename.
 * @throws {Error} When the platform/arch combo is not supported.
 */
export function resolvePlatform(platform, arch) {
  const key = `${platform}/${arch}`;
  const info = PLATFORM_MAP.get(key);
  if (info === undefined) {
    throw new Error(
      `polyplugc: no prebuilt binary for ${platform}-${arch}. ` +
      `Install with \`cargo install polyplugc\` or download from ` +
      `https://github.com/polyplug/polyplug/releases`
    );
  }
  return info;
}
