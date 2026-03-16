import { toStr, allocString } from 'polyplug-guest';

export function decode(input) {
    const s = toStr(input).replace(',', '|');
    return allocString(`DECODED:${s}`);
}

export function polyplug_init(registrar, context) {
    setJsDecoderImpl({ decode });
    return { code: 0 };
}
