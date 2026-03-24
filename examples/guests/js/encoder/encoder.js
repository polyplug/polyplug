import { setEncoderImpl } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocString } from '../../../../sdks/js/guest/polyplug_guest.js';

function encode(input) {
    const s = toStr(input);
    const data = s.startsWith("TRANSFORMED:") ? s.slice("TRANSFORMED:".length) : s;
    const result = allocString(data.replace(/\|/g, ","));
    
    const ptrLo = Number(result.ptr & 0xFFFFFFFFn);
    const ptrHi = Number((result.ptr >> 32n) & 0xFFFFFFFFn);
    
    return {
        ptr_lo: ptrLo,
        ptr_hi: ptrHi,
        len: result.len
    };
}

setEncoderImpl(encode);

export { polyplug_init };