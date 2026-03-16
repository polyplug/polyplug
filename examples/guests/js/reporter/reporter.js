import { toStr, allocString } from 'polyplug-guest';

export function report(input) {
    let s = toStr(input);
    if (s.startsWith('TRANSFORMED:')) s = s.slice(12);
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
