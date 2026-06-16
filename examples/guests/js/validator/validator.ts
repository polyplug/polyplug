import { setValidatorFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr } from '../../../../sdks/js/guest/polyplug_guest.js';

setValidatorFactory((bridge: any, hostLo: number, hostHi: number) => ({
    fn0: (input: { ptr_lo: number; ptr_hi: number; len: number }): string => {
        const s = toStr(bridge, input);
        const data = s.startsWith("DECODED:") ? s.slice("DECODED:".length) : s;
        const parts = data.split("|");

        if (parts.length === 3 && parts[0] && parts[1] && !Number.isNaN(parseInt(parts[2], 10))) {
            return `VALID:${data}`;
        }
        return "INVALID:expected name|value|count";
    }
}));

export { polyplug_init };
