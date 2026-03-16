import { toStr, allocString } from 'polyplug-guest';

export function encode(input) {
    let s = toStr(input);
    if (s.startsWith('TRANSFORMED:')) s = s.slice(12);
    return allocString(s.replace(/\|/g, ','));
}

export function polyplug_init(registrar, context) {
    setJsEncoderImpl({ encode });
    return { code: 0 };
}
