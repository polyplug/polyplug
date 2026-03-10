// THIS FILE IS PART OF THE polyplug GUEST LIBRARY FOR JAVASCRIPT/TYPESCRIPT.
// NOT auto-generated. Hand-written for the polyplug JS guest SDK.
// Shared between js-quickjs and js-deno generators.

/**
 * A read-only view into a UTF-8 string in the host's address space.
 *
 * ptr_lo/ptr_hi are the low and high 32-bit halves of the 64-bit pointer.
 * (QuickJS uses f64 internally — 64-bit integers must be split into lo/hi pairs.)
 */
export interface StringView {
    readonly ptr_lo: number;
    readonly ptr_hi: number;
    readonly len: number;
}

/**
 * A byte buffer owned by the host allocator.
 */
export interface Buffer {
    readonly ptr_lo: number;
    readonly ptr_hi: number;
    readonly len: number;
    readonly cap: number;
}

/**
 * ABI error code. code === 0 means ABI_OK (success).
 */
export interface AbiError {
    readonly code: number;
    readonly message: StringView;
}

/** ABI_OK sentinel — code 0 means success. */
export const ABI_OK: number = 0;

/**
 * Error thrown when a declared dependency cannot be resolved at init time.
 */
export class DependencyNotFoundError extends Error {
    constructor(public readonly contractName: string) {
        super(`dependency not found: ${contractName}`);
        this.name = 'DependencyNotFoundError';
    }
}

/**
 * Extension ID for the trace extension.
 * Value: fnv1a_32("trace") = 0xC4EB9AEE
 */
export const EXT_TRACE_ID: number = 0xC4EB9AEE;

/**
 * VTable interface for the trace extension.
 * Obtain via polyplug.getExtension(EXT_TRACE_ID) -> {lo, hi} | null.
 * The {lo, hi} pair is a pointer to a TraceVTable in the host.
 */
export interface TraceVTable {
    /** Emit a trace event. ptr_lo/ptr_hi/len describe a StringView of the message. */
    emit(ptr_lo: number, ptr_hi: number, len: number): void;
}

/**
 * Type mapping reference (for code generators):
 *
 * js-quickjs (QuickJS / f64 internal):   js-deno (V8 / BigInt native):
 *   u8/u16/u32   -> number               u8/u16/u32   -> number
 *   u64/i64      -> {lo:number,hi:number} u64/i64      -> bigint
 *   f32/f64      -> number               f32/f64      -> number
 *   bool         -> boolean              bool         -> boolean
 *   StringView   -> {ptr_lo,ptr_hi,len}  StringView   -> {ptr:bigint,len:number}
 *   Buffer       -> {ptr_lo,ptr_hi,len,cap} Buffer    -> {ptr:bigint,len:number,cap:number}
 *   void         -> void                 void         -> void
 */
export {}; // treat this as a module
