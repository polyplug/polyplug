import { setTransformerFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr } from '../../../../sdks/js/guest/polyplug_guest.js';

setTransformerFactory((bridge, hostLo, hostHi) => ({
    fn0: (input) => {
        const s = toStr(bridge, input);
        const data = s.startsWith('DECODED:') ? s.slice('DECODED:'.length) : s;
        const parts = data.split('|');

        if (parts.length >= 3) {
            const name = parts[0].toUpperCase();
            const value = `${parts[1]} (transformed)`;
            const count = parseInt(parts[2], 10) || 0;
            return `TRANSFORMED:${name}|${value}|${count + 1}`;
        }
        return 'INVALID:format';
    }
}));

export { polyplug_init };
