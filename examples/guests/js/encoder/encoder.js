import { allocString } from 'polyplug-guest';
import { stripPrefix } from 'polyplug-abi';

export function encode(input) {
    let s = stripPrefix(input, 'TRANSFORMED:');
    return allocString(s.replace(/\|/g, ','));
}

export function polyplug_init(registrar, context) {
    setJsEncoderImpl({ encode });
    return { code: 0 };
}
