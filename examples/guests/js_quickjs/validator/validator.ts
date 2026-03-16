// JS QuickJS validator plugin — implements pipeline.Validator@1
// Input:  "name,value,42"
// Output: "VALID:name,value,42" or error

import { setJsQuickjsValidatorImpl } from './generated/guest/contracts';

function validate(data: { ptr_lo: number; ptr_hi: number; len: number }): { ptr_lo: number; ptr_hi: number; len: number } {
    // Simple validation - in real code would validate the data
    return data;
}

setJsQuickjsValidatorImpl(validate);
