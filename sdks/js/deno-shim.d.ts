// Minimal ambient Deno declaration used ONLY to satisfy `tsc` when transpiling
// the npm distribution of the Deno-targeted packages. The real Deno runtime
// supplies these globals at execution time; this shim is a build-time type aid.
// The jsr/Deno build path uses Deno's own bundled types and never sees this file.
//
// It is intentionally loose (`any`) — the goal is a clean transpile to `.js`
// + `.d.ts`, not full Deno type coverage. It declares both the value `Deno`
// (for `Deno.env`, `Deno.build`, ...) and the namespace `Deno` (for type
// positions like `Deno.DynamicLibrary`, `Deno.ForeignFunction`).
declare const Deno: any;
declare namespace Deno {
  type DynamicLibrary<T> = any;
  type ForeignFunction = any;
  type ForeignLibraryInterface = any;
  type UnsafePointer = any;
}
