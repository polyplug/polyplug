/**
 * @file mod.ts
 * @description Main entry point for the polyplug JavaScript/TypeScript SDK.
 */

export * from "./abi/polyplug_abi.ts";
export {
    Runtime,
    openPolyplug,
    runtimeNew,
    onReload,
    setConfig,
    NULL_HANDLE,
    fnv1a64,
    COMPATIBILITY_STRICT,
    COMPATIBILITY_RELAXED,
    COMPATIBILITY_YOLO,
} from "./host/mod.js";
export { ReloadPhase } from "./host/polyplug/reload_phase.js";
export {
    getPlatformIdentifier,
    getNativeLibraryFilename,
    loadNativeLibrary,
    openNativeLibrary,
} from "./host/polyplug/native-loader.ts";
export { DependencyNotFoundError } from "./guest/polyplug_guest.js";