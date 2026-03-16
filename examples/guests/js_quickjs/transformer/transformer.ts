// JS QuickJS transformer plugin — implements data.Transformer@1
// Input:  "name,value,42"
// Output: "TRANSFORMED:name|value|42"

import { setJsQuickjsTransformerImpl } from './generated/guest/contracts';

function transform(data: { ptr_lo: number; ptr_hi: number; len: number }): { ptr_lo: number; ptr_hi: number; len: number } {
    return data;
}

setJsQuickjsTransformerImpl(transform);
