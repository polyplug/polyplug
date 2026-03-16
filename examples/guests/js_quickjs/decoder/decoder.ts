// JS QuickJS decoder plugin — implements pipeline.Decoder@1
// Input:  "name,value,42"
// Output: "DECODED:name|value|42"

import { setJsQuickjsDecoderImpl } from './generated/guest/contracts';

function decode(input: { ptr_lo: number; ptr_hi: number; len: number }): { ptr_lo: number; ptr_hi: number; len: number } {
    // Simple implementation - in real code would decode the input
    return input;
}

// Register implementation
setJsQuickjsDecoderImpl(decode);
