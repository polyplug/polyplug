// decoder.js - Plugin implementation using generated code
// Bundled with rolldown along with generated code

import { setDecoderImpl } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocStringArena } from '../../../../sdks/js/guest/polyplug_guest.js';

/**
 * Decode function implementation.
 * Converts comma-separated input to pipe-separated output.
 * 
 * @param {{ ptr_lo: number; ptr_hi: number; len: number }} input - StringView input
 * @returns {{ ptr_lo: number; ptr_hi: number; len: number }} - StringView output
 */
function decode(input) {
    const s = toStr(input);
    const decoded = s.replace(/,/g, '|');
    const result = allocStringArena(`DECODED:${decoded}`);
    
    const ptrLo = Number(result.ptr & 0xFFFFFFFFn);
    const ptrHi = Number((result.ptr >> 32n) & 0xFFFFFFFFn);
    
    return {
        ptr_lo: ptrLo,
        ptr_hi: ptrHi,
        len: result.len
    };
}

setDecoderImpl(decode);

export { polyplug_init };