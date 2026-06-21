/**
 * @file native-loader.ts
 * @description Platform detection and native library loading for polyplug.
 * 
 * This module provides automatic platform detection and loads the appropriate
 * native library from the embedded _native/ directory or from environment
 * variable overrides.
 * 
 * @module polyplug/native-loader
 */

/**
 * Platform identifier format: {os}-{arch}
 * Examples: "linux-x64", "macos-arm64", "windows-x64"
 */
export type PlatformIdentifier =
  | "linux-x64"
  | "macos-x64"
  | "macos-arm64"
  | "windows-x64";

/**
 * Mapping of Deno OS names (Deno.build.os) to platform-directory segments.
 */
const OS_MAP: Record<string, string> = {
  "linux": "linux",
  "darwin": "macos",
  "windows": "windows"
};

/**
 * Mapping of Deno arch names to platform identifiers
 */
const ARCH_MAP: Record<string, string> = {
  "x86_64": "x64",
  "aarch64": "arm64"
};

/**
 * Resolve a platform identifier from raw Deno OS/arch names.
 * Pure function (no Deno.build access) so every OS/arch mapping is testable.
 * @param {string} os   A Deno.build.os value ("linux", "darwin", "windows").
 * @param {string} arch A Deno.build.arch value ("x86_64", "aarch64").
 * @returns {PlatformIdentifier} Platform identifier string.
 * @throws {Error} If the OS or architecture is not supported.
 */
export function platformFor(os: string, arch: string): PlatformIdentifier {
  const osName = OS_MAP[os];
  const archName = ARCH_MAP[arch];

  if (!osName) {
    throw new Error(`Unsupported OS: ${os}`);
  }

  if (!archName) {
    throw new Error(`Unsupported architecture: ${arch}`);
  }

  const platform = `${osName}-${archName}` as PlatformIdentifier;

  const validPlatforms: PlatformIdentifier[] = [
    "linux-x64",
    "macos-x64",
    "macos-arm64",
    "windows-x64"
  ];

  if (!validPlatforms.includes(platform)) {
    throw new Error(`Unsupported platform combination: ${os}-${arch}`);
  }

  return platform;
}

/**
 * Get the platform identifier based on current Deno build.
 * @returns {PlatformIdentifier} Platform identifier string
 * @throws {Error} If platform is not supported
 */
export function getPlatformIdentifier(): PlatformIdentifier {
  return platformFor(Deno.build.os, Deno.build.arch);
}

/**
 * Resolve the native library filename for a raw Deno OS name.
 * Pure function (no Deno.build access) so every OS mapping is testable.
 * @param {string} os A Deno.build.os value ("linux", "darwin", "windows").
 * @returns {string} Native library filename.
 * @throws {Error} If the OS is not supported.
 */
export function nativeLibraryFilenameFor(os: string): string {
  switch (os) {
    case "windows":
      return "polyplug.dll";
    case "darwin":
      return "libpolyplug.dylib";
    case "linux":
      return "libpolyplug.so";
    default:
      throw new Error(`Unsupported OS: ${os}`);
  }
}

/**
 * Get the native library filename for the current platform.
 * @returns {string} Native library filename
 */
export function getNativeLibraryFilename(): string {
  return nativeLibraryFilenameFor(Deno.build.os);
}

/**
 * Options for loading the native library
 */
export interface LoadNativeOptions {
  /**
   * Override the platform identifier (for testing)
   */
  platform?: PlatformIdentifier;
  
  /**
   * Override the library path (for development)
   * If provided, this takes precedence over embedded libraries
   */
  libPath?: string;
}

/**
 * Result of loading the native library
 */
export interface NativeLibraryResult {
  /**
   * Path to the loaded library
   */
  path: string;
  
  /**
   * Whether the library was loaded from embedded location
   */
  isEmbedded: boolean;
  
  /**
   * Platform identifier used
   */
  platform: PlatformIdentifier;
}

/**
 * Load the native polyplug library with automatic platform detection.
 * 
 * Priority order:
 * 1. POLYPLUG_LIB environment variable (if set)
 * 2. Embedded library from _native/{platform}/
 * 3. System library paths
 * 
 * @param {LoadNativeOptions} [options] - Loading options
 * @returns {NativeLibraryResult} Information about the loaded library
 * @throws {Error} If library cannot be found or loaded
 * 
 * @example
 * ```typescript
 * // Automatic platform detection
 * const { path, isEmbedded, platform } = loadNativeLibrary();
 * console.log(`Loaded from: ${path} (${platform})`);
 * 
 * // Override for testing
 * const result = loadNativeLibrary({ platform: "linux-x64" });
 * 
 * // Use custom library path
 * const result = loadNativeLibrary({ libPath: "/usr/local/lib/libpolyplug.so" });
 * ```
 */
export function loadNativeLibrary(options?: LoadNativeOptions): NativeLibraryResult {
  // Check for environment variable override first
  const envLibPath = Deno.env.get("POLYPLUG_LIB");
  if (envLibPath) {
    return {
      path: envLibPath,
      isEmbedded: false,
      platform: getPlatformIdentifier()
    };
  }
  
  // Check for explicit lib path override
  if (options?.libPath) {
    return {
      path: options.libPath,
      isEmbedded: false,
      platform: getPlatformIdentifier()
    };
  }
  
  // Use embedded library
  const platform = options?.platform || getPlatformIdentifier();
  const libName = getNativeLibraryFilename();
  const libPath = new URL(`./_native/${platform}/${libName}`, import.meta.url);
  
  // Verify the library file exists
  try {
    Deno.statSync(libPath);
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) {
      throw new Error(
        `Native library not found for ${platform}. ` +
        `Expected at: ${libPath.pathname}\n` +
        `Run 'deno run --allow-net --allow-write download-native.ts' to download, ` +
        `or set POLYPLUG_LIB environment variable.`
      );
    }
    throw error;
  }
  
  return {
    path: libPath.pathname,
    isEmbedded: true,
    platform
  };
}

/**
 * Open the native library and return a DynamicLibrary instance.
 * This is a convenience wrapper around loadNativeLibrary() + Deno.dlopen().
 * 
 * @param {LoadNativeOptions} [options] - Loading options
 * @param {Record<string, Deno.ForeignFunction>} symbols - Symbol definitions
 * @returns {Deno.DynamicLibrary} Dynamic library instance
 * @throws {Error} If library cannot be loaded
 * 
 * @example
 * ```typescript
 * const lib = openNativeLibrary({
 *   polyplug_runtime_create: { parameters: [], result: "pointer" }
 * });
 * ```
 */
export function openNativeLibrary<T extends Record<string, Deno.ForeignFunction>>(
  symbols: T,
  options?: LoadNativeOptions
): Deno.DynamicLibrary<T> {
  const { path } = loadNativeLibrary(options);
  return Deno.dlopen(path, symbols);
}
