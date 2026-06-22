// sdks/js/testing/harness.ts
// A zero-dependency, runtime-agnostic test harness for the polyplug JS SDK.
//
// # Why this exists
//
// The SDK test suite must run identically under Deno, Node, and Bun. Deno's
// built-in `Deno.test` harness and the JSR-only `@std/assert` package are both
// Deno-specific, so the suite cannot depend on either. This module provides a
// minimal collector (`test`), faithful re-implementations of the three
// `@std/assert` assertions the suite uses, and a runner that executes every
// registered test and reports a pass/fail summary.
//
// The registry below is TEST infrastructure — it collects test cases for a
// single runner invocation. It holds no polyplug runtime or plugin state, so
// Rule 12 (no runtime/plugin globals) does not apply to it.

/** A single registered test: a name and the body to run. */
interface RegisteredTest {
    readonly name: string;
    readonly fn: () => void | Promise<void>;
}

/** The result of a full runner pass. */
export interface TestRunResult {
    readonly passed: number;
    readonly failed: number;
}

const registry: RegisteredTest[] = [];

/**
 * Register a test case. Mirrors `Deno.test(name, fn)`: the body may be sync or
 * async. Registration is deferred — nothing runs until {@link runRegisteredTests}.
 */
export function test(name: string, fn: () => void | Promise<void>): void {
    registry.push({ name, fn });
}

/** Thrown by the assertion helpers when an expectation fails. */
export class AssertionError extends Error {
    constructor(message: string) {
        super(message);
        this.name = "AssertionError";
    }
}

/**
 * Deep structural equality for assertion comparison. Re-implements the subset of
 * `@std/assert`'s `equal` semantics the suite relies on: primitives (including
 * `bigint`) by `Object.is`, arrays and plain objects by recursive member
 * equality, and typed arrays / `ArrayBuffer` by byte content.
 */
function deepEqual(a: unknown, b: unknown): boolean {
    if (Object.is(a, b)) {
        return true;
    }
    if (typeof a !== typeof b) {
        return false;
    }
    if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
        return false;
    }

    if (a instanceof ArrayBuffer && b instanceof ArrayBuffer) {
        return bytesEqual(new Uint8Array(a), new Uint8Array(b));
    }
    if (ArrayBuffer.isView(a) && ArrayBuffer.isView(b)) {
        return bytesEqual(toBytes(a), toBytes(b));
    }

    if (Array.isArray(a) || Array.isArray(b)) {
        if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
            return false;
        }
        for (let i = 0; i < a.length; i++) {
            if (!deepEqual(a[i], b[i])) {
                return false;
            }
        }
        return true;
    }

    const aObj: Record<string, unknown> = a as Record<string, unknown>;
    const bObj: Record<string, unknown> = b as Record<string, unknown>;
    const aKeys: string[] = Object.keys(aObj);
    const bKeys: string[] = Object.keys(bObj);
    if (aKeys.length !== bKeys.length) {
        return false;
    }
    for (const key of aKeys) {
        if (!Object.prototype.hasOwnProperty.call(bObj, key)) {
            return false;
        }
        if (!deepEqual(aObj[key], bObj[key])) {
            return false;
        }
    }
    return true;
}

function toBytes(view: ArrayBufferView): Uint8Array {
    return new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
    if (a.byteLength !== b.byteLength) {
        return false;
    }
    for (let i = 0; i < a.byteLength; i++) {
        if (a[i] !== b[i]) {
            return false;
        }
    }
    return true;
}

function stringify(value: unknown): string {
    if (typeof value === "bigint") {
        return `${value}n`;
    }
    if (typeof value === "string") {
        return JSON.stringify(value);
    }
    try {
        return String(value);
    } catch {
        return "<unrepresentable>";
    }
}

/**
 * Assert that `actual` deeply equals `expected`. Mirrors `@std/assert`'s
 * `assertEquals` for the value shapes the suite uses.
 */
export function assertEquals<T>(actual: T, expected: T, msg?: string): void {
    if (!deepEqual(actual, expected)) {
        const detail: string = `Values are not equal.\n  actual:   ${
            stringify(actual)
        }\n  expected: ${stringify(expected)}`;
        throw new AssertionError(msg ? `${msg}\n${detail}` : detail);
    }
}

/**
 * Assert that `actual` is strictly (`Object.is`) the same as `expected`.
 * Mirrors `@std/assert`'s `assertStrictEquals`.
 */
export function assertStrictEquals<T>(actual: T, expected: T, msg?: string): void {
    if (!Object.is(actual, expected)) {
        const detail: string = `Values are not strictly equal.\n  actual:   ${
            stringify(actual)
        }\n  expected: ${stringify(expected)}`;
        throw new AssertionError(msg ? `${msg}\n${detail}` : detail);
    }
}

/**
 * Assert that `fn` throws. When `ErrorClass` is given the thrown value must be an
 * instance of it; when `msgIncludes` is given the thrown error's message must
 * contain that substring. Mirrors `@std/assert`'s `assertThrows(fn, ErrorClass?,
 * msgIncludes?)`. Returns the thrown error for further inspection.
 */
export function assertThrows(
    fn: () => unknown,
    ErrorClass?: abstract new (...args: never[]) => Error,
    msgIncludes?: string,
): Error {
    let thrown: unknown;
    let didThrow = false;
    try {
        fn();
    } catch (error) {
        didThrow = true;
        thrown = error;
    }
    if (!didThrow) {
        throw new AssertionError("Expected function to throw, but it did not.");
    }
    if (ErrorClass !== undefined && !(thrown instanceof ErrorClass)) {
        throw new AssertionError(
            `Expected error to be an instance of ${ErrorClass.name}, got: ${stringify(thrown)}`,
        );
    }
    if (!(thrown instanceof Error)) {
        throw new AssertionError(
            `Expected a thrown Error, got: ${stringify(thrown)}`,
        );
    }
    if (msgIncludes !== undefined && !thrown.message.includes(msgIncludes)) {
        throw new AssertionError(
            `Expected error message to include ${stringify(msgIncludes)}, got: ${
                stringify(thrown.message)
            }`,
        );
    }
    return thrown;
}

/**
 * Run every registered test in registration order, printing a per-test `ok` /
 * `FAIL` line and a final summary. Returns the pass/fail counts so the
 * entrypoint can set a non-zero exit code. The registry is cleared afterwards so
 * a process that runs the runner twice does not double-count.
 */
export async function runRegisteredTests(): Promise<TestRunResult> {
    let passed = 0;
    let failed = 0;
    for (const entry of registry) {
        try {
            await entry.fn();
            passed++;
            console.log(`ok    ${entry.name}`);
        } catch (error) {
            failed++;
            const message: string = error instanceof Error
                ? `${error.message}\n${error.stack ?? ""}`
                : String(error);
            console.error(`FAIL  ${entry.name}\n${message}`);
        }
    }
    registry.length = 0;
    console.log(`\n${passed} passed, ${failed} failed`);
    return { passed, failed };
}
