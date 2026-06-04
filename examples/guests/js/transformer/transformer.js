import { setTransformerImpl } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocString } from '../../../../sdks/js/guest/polyplug_guest.js';

function transform(input) {
    const s = toStr(input);
    const data = s.startsWith('DECODED:') ? s.slice('DECODED:'.length) : s;
    const parts = data.split('|');
    
    if (parts.length >= 3) {
        const name = parts[0].toUpperCase();
        const value = `${parts[1]} (transformed)`;
        const count = parseInt(parts[2], 10) || 0;
        const result = allocString(`TRANSFORMED:${name}|${value}|${count + 1}`);
        
        const ptrLo = Number(result.ptr & 0xFFFFFFFFn);
        const ptrHi = Number((result.ptr >> 32n) & 0xFFFFFFFFn);
        
        return {
            ptr_lo: ptrLo,
            ptr_hi: ptrHi,
            len: result.len
        };
    }
    
    return { ptr_lo: 0, ptr_hi: 0, len: 0 };
}

setTransformerImpl(transform);

export { polyplug_init };