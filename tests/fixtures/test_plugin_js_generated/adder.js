// tests/fixtures/test_plugin_js_generated/adder.js
// Source for the GENERATED-glue QuickJS fixture: implements test.add@1.0 via
// the polyplugc-generated wrappers (generated/guest/contracts.ts) instead of
// hand-rolled ABI code (compare test_plugin_js/bundle.js). The runtime test
// integration_js_generated_guest.rs drives every non-StringView signature
// shape through these wrappers. Rebuilt by tests/fixtures/build_all.sh.

import { setTestAdderFactory } from './generated/guest/contracts';
import { POLYPLUG_MANIFEST, polyplug_init } from './generated/guest/init';

// The factory receives the bridge + host vtable lo/hi explicitly (no global —
// Rule 12). A StringView-returning method returns a plain string; the generated
// wrapper arena-allocates it.
setTestAdderFactory((bridge, hostLo, hostHi) => ({
    // fn0: add(args: AddArgs { a: u32, b: u32 }) -> u32  (struct-by-value param)
    fn0: (args) => (args.a + args.b) >>> 0,
    // fn1: add_primitive(a: u32, b: u32) -> u32  (multi-scalar pack)
    fn1: (a, b) => (a + b) >>> 0,
    // fn2: version() -> StringView  (no args, string return)
    fn2: () => 'test_adder 1.0.0',
    // fn3: reset() -> void  (no args, no return)
    fn3: () => {},
}));

export { POLYPLUG_MANIFEST, polyplug_init };
