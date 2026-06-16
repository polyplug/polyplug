// decoder.js - Plugin implementation using generated code
// Bundled with rolldown along with generated code

import { setDecoderFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr } from '../../../../sdks/js/guest/polyplug_guest.js';

// Factory returning a per-instance decoder. The loader calls this once per
// instance (and once at load for the stateless default impl), threading the
// bridge + host vtable explicitly (no global — Rule 12); the impl captures the
// bridge so its methods reach host capabilities through it.
setDecoderFactory((bridge, hostLo, hostHi) => ({
    // Convert comma-separated input to pipe-separated output. The author returns
    // a plain string; the generated wrapper arena-allocates it.
    fn0: (input) => {
        const s = toStr(bridge, input);
        return `DECODED:${s.replace(/,/g, '|')}`;
    }
}));

export { polyplug_init };
