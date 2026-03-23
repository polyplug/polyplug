import { allocString } from 'polyplug-guest';
import { stripPrefix } from 'polyplug-abi';

export function report(input) {
    let s = stripPrefix(input, 'TRANSFORMED:');
    const parts = s.split('|');
    if (parts.length >= 3) {
        return allocString(`Report: ${parts[0]} has value '${parts[1]}' with count ${parts[2]}`);
    }
    return allocString('INVALID:format');
}

export function polyplug_init(registrar, context) {
    setJsReporterImpl({ report });
    return { code: 0 };
}
