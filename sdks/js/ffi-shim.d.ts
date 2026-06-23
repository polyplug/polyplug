// Minimal ambient declarations for the runtime-only FFI modules `koffi` (Node)
// and `bun:ffi` (Bun), used ONLY to satisfy `tsc` when transpiling the npm
// distribution of `@polyplug/abi`. The real modules are supplied at execution
// time — `koffi` as an optional npm dependency installed under Node, and
// `bun:ffi` as a built-in under Bun. This shim is a build-time type aid so the
// distribution transpiles to `.js` + `.d.ts` with no installed dependency, the
// same role `deno-shim.d.ts` plays for the `Deno` globals.
//
// Both backends (`ffi/node.ts`, `ffi/bun.ts`) cast every imported value through
// `as unknown` into a local `*FfiApi` interface that fully describes the surface
// they use, so the imported types are never load-bearing. The shims are
// intentionally loose (`any`) — the goal is a clean transpile, not type coverage.
declare module "koffi" {
  const koffi: any;
  export default koffi;
}

declare module "bun:ffi" {
  export const CFunction: any;
  export const dlopen: any;
  export const JSCallback: any;
  export const ptr: any;
  export const read: any;
  export const toArrayBuffer: any;
}
