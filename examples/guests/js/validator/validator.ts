import { setValidatorFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocString } from '../../../../sdks/js/guest/polyplug_guest.js';

function validate(input: { ptr_lo: number; ptr_hi: number; len: number }): { ptr_lo: number; ptr_hi: number; len: number } {
    const s = toStr(input);
    const data = s.startsWith("DECODED:") ? s.slice("DECODED:".length) : s;
    const parts = data.split("|");

    let result;
    if (parts.length === 3 && parts[0] && parts[1] && !Number.isNaN(parseInt(parts[2], 10))) {
        result = allocString(`VALID:${data}`);
    } else {
        result = allocString("INVALID:expected name|value|count");
    }

    const ptrLo = Number(result.ptr & 0xFFFFFFFFn);
    const ptrHi = Number((result.ptr >> 32n) & 0xFFFFFFFFn);
    return { ptr_lo: ptrLo, ptr_hi: ptrHi, len: result.len };
}

setValidatorFactory(() => ({ fn0: validate }));

export { polyplug_init };
