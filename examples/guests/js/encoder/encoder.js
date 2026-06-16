import { setEncoderFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr } from '../../../../sdks/js/guest/polyplug_guest.js';

setEncoderFactory((bridge, hostLo, hostHi) => ({
    fn0: (input) => {
        const s = toStr(bridge, input);
        const data = s.startsWith("TRANSFORMED:") ? s.slice("TRANSFORMED:".length) : s;
        return data.replace(/\|/g, ",");
    }
}));

export { polyplug_init };
