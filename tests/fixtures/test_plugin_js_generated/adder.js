// tests/fixtures/test_plugin_js_generated/adder.js
// Source for the GENERATED-glue QuickJS fixture: implements test.add@1.0 via
// the polyplugc-generated wrappers (generated/guest/contracts.ts) instead of
// hand-rolled ABI code (compare test_plugin_js/bundle.js). The runtime test
// integration_js_generated_guest.rs drives every non-StringView signature
// shape through these wrappers. Rebuilt by tests/fixtures/build_all.sh.

import { setTestAdderFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { allocStringArena } from '../../../sdks/js/guest/polyplug_guest.js';

// fn0: add(args: AddArgs { a: u32, b: u32 }) -> u32  (struct-by-value param)
function add(args) {
    return (args.a + args.b) >>> 0;
}

// fn1: add_primitive(a: u32, b: u32) -> u32  (multi-scalar pack)
function addPrimitive(a, b) {
    return (a + b) >>> 0;
}

// fn2: version() -> StringView  (no args, string return)
function version() {
    const result = allocStringArena('test_adder 1.0.0');
    return {
        ptr_lo: Number(result.ptr & 0xFFFFFFFFn),
        ptr_hi: Number((result.ptr >> 32n) & 0xFFFFFFFFn),
        len: result.len,
    };
}

// fn3: reset() -> void  (no args, no return)
function reset() {
}

setTestAdderFactory(() => ({ fn0: add, fn1: addPrimitive, fn2: version, fn3: reset }));

export { polyplug_init };
