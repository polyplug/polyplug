import { setReporterFactory } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr } from '../../../../sdks/js/guest/polyplug_guest.js';

setReporterFactory((bridge, hostLo, hostHi) => ({
    fn0: (input) => {
        const s = toStr(bridge, input);
        const data = s.startsWith("TRANSFORMED:") ? s.slice("TRANSFORMED:".length) : s;
        const parts = data.split("|");

        if (parts.length >= 3) {
            return `Report: ${parts[0]} has value '${parts[1]}' with count ${parts[2]}`;
        }
        return "INVALID:format";
    }
}));

export { polyplug_init };
