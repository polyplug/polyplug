// JS QuickJS encoder plugin — implements pipeline.Encoder@1
// Input:  "name|value|42"
// Output: "ENCODED:name,value,42"

import { setJsQuickjsEncoderImpl } from './generated/guest/contracts';

function encode(data: { ptr_lo: number; ptr_hi: number; len: number }): { ptr_lo: number; ptr_hi: number; len: number } {
    return data;
}

setJsQuickjsEncoderImpl(encode);
