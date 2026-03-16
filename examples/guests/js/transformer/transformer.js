import { toStr, allocString } from 'polyplug-guest';

export function transform(input) {
    let s = toStr(input);
    if (s.startsWith('DECODED:')) s = s.slice(8);
    const parts = s.split('|');
    if (parts.length >= 3) {
        const name = parts[0].toUpperCase();
        const value = `${parts[1]} (transformed)`;
        const count = parseInt(parts[2]) + 1;
        return allocString(`TRANSFORMED:${name}|${value}|${count}`);
    }
    return allocString('INVALID:format');
}

export function polyplug_init(registrar, context) {
    setJsTransformerImpl({ transform });
    return { code: 0 };
}
