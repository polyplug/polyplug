#!/usr/bin/env -S deno run --allow-net --allow-write --allow-env

/**
 * @file download-native.ts
 * @description Download native polyplug libraries from GitHub Releases.
 * 
 * This script downloads the appropriate native library for each supported
 * platform from GitHub Releases and places them in the _native/ directory.
 * 
 * Usage:
 *   deno run --allow-net --allow-write --allow-env download-native.ts
 * 
 * Environment variables:
 *   POLYPLUG_VERSION - Version to download (default: "0.1.0")
 *   GITHUB_TOKEN - Optional token for higher rate limits
 * 
 * @module polyplug/download-native
 */

import { getPlatformIdentifier, getNativeLibraryFilename } from "./native-loader.ts";

/**
 * Platform configuration for downloads
 */
interface PlatformConfig {
  /** Platform identifier */
  platform: string;
  /** GitHub Releases asset filename */
  assetName: string;
  /** Local filename (may differ from asset name on some platforms) */
  localName: string;
}

/**
 * Supported platforms and their download configurations
 */
const PLATFORMS: PlatformConfig[] = [
  {
    platform: "linux-x64",
    assetName: "libpolyplug-linux-x64.so",
    localName: "libpolyplug.so"
  },
  {
    platform: "darwin-x64",
    assetName: "libpolyplug-macos-x64.dylib",
    localName: "libpolyplug.dylib"
  },
  {
    platform: "darwin-arm64",
    assetName: "libpolyplug-macos-arm64.dylib",
    localName: "libpolyplug.dylib"
  },
  {
    platform: "win32-x64",
    assetName: "polyplug-windows-x64.dll",
    localName: "polyplug.dll"
  }
];

/**
 * Get the version to download from environment or use default
 */
function getVersion(): string {
  return Deno.env.get("POLYPLUG_VERSION") || "0.1.0";
}

/**
 * Get GitHub token from environment (optional, for rate limits)
 */
function getGithubToken(): string | undefined {
  return Deno.env.get("GITHUB_TOKEN");
}

/**
 * Build GitHub Releases URL for a specific asset
 */
function getReleaseAssetUrl(version: string, assetName: string): string {
  return `https://github.com/polyplug/polyplug/releases/download/v${version}/${assetName}`;
}

/**
 * Download a file from URL with progress reporting
 */
async function downloadFile(
  url: string,
  outputPath: string,
  token?: string
): Promise<void> {
  const headers: HeadersInit = {
    "Accept": "application/octet-stream"
  };
  
  if (token) {
    headers["Authorization"] = `token ${token}`;
  }
  
  console.log(`  Downloading: ${url}`);
  
  const response = await fetch(url, { headers });
  
  if (!response.ok) {
    if (response.status === 404) {
      throw new Error(`Asset not found (404): ${new URL(url).pathname.split('/').pop()}`);
    }
    if (response.status === 403) {
      throw new Error(
        `Rate limited (403). Set GITHUB_TOKEN environment variable.\n` +
        `Get a token at: https://github.com/settings/tokens`
      );
    }
    throw new Error(`Download failed: ${response.status} ${response.statusText}`);
  }
  
  const contentLength = response.headers.get("content-length");
  const totalSize = contentLength ? parseInt(contentLength, 10) : null;
  
  const data = await response.arrayBuffer();
  const bytes = new Uint8Array(data);
  
  // Ensure directory exists
  await Deno.mkdir(new URL(".", outputPath), { recursive: true });
  
  // Write file
  await Deno.writeFile(outputPath, bytes);
  
  const sizeKb = (bytes.length / 1024).toFixed(2);
  console.log(`  Saved: ${outputPath} (${sizeKb} KB)`);
}

/**
 * Download all native libraries for all platforms
 */
async function downloadAll(): Promise<void> {
  const version = getVersion();
  const token = getGithubToken();
  
  console.log(`Downloading polyplug native libraries v${version}`);
  console.log(`GitHub Releases: https://github.com/polyplug/polyplug/releases/tag/v${version}`);
  console.log("");
  
  let successCount = 0;
  let failCount = 0;
  
  for (const config of PLATFORMS) {
    const url = getReleaseAssetUrl(version, config.assetName);
    const outputPath = new URL(`./_native/${config.platform}/${config.localName}`, import.meta.url);
    
    console.log(`[${config.platform}]`);
    
    try {
      await downloadFile(url, outputPath.pathname, token);
      successCount++;
    } catch (error) {
      failCount++;
      console.error(`  ERROR: ${(error as Error).message}`);
    }
    
    console.log("");
  }
  
  console.log(`Download complete: ${successCount} succeeded, ${failCount} failed`);
  
  if (failCount > 0) {
    Deno.exit(1);
  }
}

/**
 * Download only the library for the current platform
 */
async function downloadCurrent(): Promise<void> {
  const version = getVersion();
  const token = getGithubToken();
  const platform = getPlatformIdentifier();
  
  const config = PLATFORMS.find(p => p.platform === platform);
  if (!config) {
    console.error(`Unsupported platform: ${platform}`);
    Deno.exit(1);
  }
  
  const url = getReleaseAssetUrl(version, config.assetName);
  const outputPath = new URL(`./_native/${config.platform}/${config.localName}`, import.meta.url);
  
  console.log(`Downloading polyplug native library v${version} for ${platform}`);
  console.log("");
  
  try {
    await downloadFile(url, outputPath.pathname, token);
    console.log("");
    console.log("Download complete!");
  } catch (error) {
    console.error(`ERROR: ${(error as Error).message}`);
    Deno.exit(1);
  }
}

/**
 * Main entry point
 */
async function main(): Promise<void> {
  const args = Deno.args;
  
  if (args.includes("--help") || args.includes("-h")) {
    console.log(`
Download native polyplug libraries from GitHub Releases

Usage:
  deno run --allow-net --allow-write --allow-env download-native.ts [OPTIONS]

Options:
  --current, -c    Download only for current platform
  --all, -a        Download for all platforms (default)
  --help, -h       Show this help message

Environment variables:
  POLYPLUG_VERSION    Version to download (default: "0.1.0")
  GITHUB_TOKEN        GitHub token for higher rate limits

Examples:
  # Download all platforms
  deno run --allow-net --allow-write --allow-env download-native.ts
  
  # Download only current platform
  deno run --allow-net --allow-write --allow-env download-native.ts --current
  
  # Download specific version
  POLYPLUG_VERSION=0.2.0 deno run --allow-net --allow-write --allow-env download-native.ts
`);
    return;
  }
  
  if (args.includes("--current") || args.includes("-c")) {
    await downloadCurrent();
  } else {
    await downloadAll();
  }
}

// Run main
main();
