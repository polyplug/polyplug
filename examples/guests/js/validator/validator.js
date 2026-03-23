import { allocString } from 'polyplug-guest';
import { stripPrefix } from 'polyplug-abi';

export function validate(input) {
    let s = stripPrefix(input, 'DECODED:');
    const parts = s.split('|');
    if (parts.length === 3 && parts[0] && parts[1] && !isNaN(parseInt(parts[2]))) {
        return allocString(`VALID:${s}`);
    }
    return allocString('INVALID:expected name|value|count');
}

export function polyplug_init(registrar, context) {
    setJsValidatorImpl({ validate });
    return { code: 0 };
}
