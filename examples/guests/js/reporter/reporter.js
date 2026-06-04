import { setReporterImpl } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocString } from '../../../../sdks/js/guest/polyplug_guest.js';

function report(input) {
    const s = toStr(input);
    const data = s.startsWith("TRANSFORMED:") ? s.slice("TRANSFORMED:".length) : s;
    const parts = data.split("|");

    let result;
    if (parts.length >= 3) {
        result = allocString(`Report: ${parts[0]} has value '${parts[1]}' with count ${parts[2]}`);
    } else {
        result = allocString("INVALID:format");
    }

    const ptrLo = Number(result.ptr & 0xFFFFFFFFn);
    const ptrHi = Number((result.ptr >> 32n) & 0xFFFFFFFFn);
    return { ptr_lo: ptrLo, ptr_hi: ptrHi, len: result.len };
}

setReporterImpl(report);

export { polyplug_init };
