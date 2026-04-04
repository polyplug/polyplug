/**
 * @file mod.ts
 * @description Main entry point for the polyplug JavaScript/TypeScript SDK.
 */

export * from "./abi/polyplug_abi.ts";
export { Runtime, openPolyplug, runtimeNew, onReload, setConfig, NULL_HANDLE, fnv1a64 } from "./host/mod.js";
export { ReloadPhase } from "./host/polyplug/reload_phase.js";
export {
    getPlatformIdentifier,
    getNativeLibraryFilename,
    loadNativeLibrary,
    openNativeLibrary,
} from "./host/polyplug/native-loader.ts";
export {
    DependencyNotFoundError,
    getExtension,
    EXT_TRACE_ID,
} from "./guest/polyplug_guest.js";