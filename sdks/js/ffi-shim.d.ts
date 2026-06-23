// Minimal ambient declarations for the modules the FFI backends import, used
// ONLY to satisfy `tsc` when transpiling the npm distribution of `@polyplug/abi`
// with NO installed dependencies (the publish path in release.yml and the
// install-smoke both run `tsc` dep-free). The real modules are supplied at
// execution time — `koffi` as an optional npm dependency installed under Node,
// `bun:ffi` as a built-in under Bun, and `node:module` / `node:fs` as Node
// builtins (typed by `@types/node` during local dev / `deno check`, absent in the
// dep-free transpile). This shim is a build-time type aid, the same role
// `deno-shim.d.ts` plays for the `Deno` globals.
//
// Both backends (`ffi/node.ts`, `ffi/bun.ts`) cast every imported value through
// `as unknown` into a local `*FfiApi` interface that fully describes the surface
// they use, so the imported types are never load-bearing. The shims are
// intentionally loose (`any`) — the goal is a clean transpile, not type coverage.
declare module "koffi" {
  const koffi: any;
  export default koffi;
}

// Node builtins used by the lazy backend loader (`ffi/index.ts` → createRequire)
// and the Node/Bun backends (`ffi/{node,bun}.ts` → statSync). Loose `any` so they
// merge harmlessly with `@types/node` when it is present (skipLibCheck is on).
declare module "node:module" {
  export const createRequire: any;
}

declare module "node:fs" {
  export const statSync: any;
}

declare module "node:buffer" {
  export const Buffer: any;
  // node.ts annotates locals as `: Buffer`, so the name is needed in type
  // position too (the value `any` alone is not a type).
  export type Buffer = any;
}

declare module "node:process" {
  const process: any;
  export default process;
}

declare module "bun:ffi" {
  export const CFunction: any;
  export const dlopen: any;
  export const JSCallback: any;
  export const ptr: any;
  export const read: any;
  export const toArrayBuffer: any;
}
