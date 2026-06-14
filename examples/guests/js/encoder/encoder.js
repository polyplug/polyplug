import { setEncoderFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocStringArena } from '../../../../sdks/js/guest/polyplug_guest.js';

function encode(input) {
    const s = toStr(input);
    const data = s.startsWith("TRANSFORMED:") ? s.slice("TRANSFORMED:".length) : s;
    const result = allocStringArena(data.replace(/\|/g, ","));
    
    const ptrLo = Number(result.ptr & 0xFFFFFFFFn);
    const ptrHi = Number((result.ptr >> 32n) & 0xFFFFFFFFn);
    
    return {
        ptr_lo: ptrLo,
        ptr_hi: ptrHi,
        len: result.len
    };
}

setEncoderFactory(() => ({ fn0: encode }));

export { polyplug_init };