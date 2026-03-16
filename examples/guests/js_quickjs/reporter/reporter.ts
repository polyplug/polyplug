// JS QuickJS reporter plugin — implements data.Reporter@1
// Input:  "name,value,42"
// Output: "REPORTED:name|value|42"

import { setJsQuickjsReporterImpl } from './generated/guest/contracts';

function report(data: { ptr_lo: number; ptr_hi: number; len: number }): { ptr_lo: number; ptr_hi: number; len: number } {
    return data;
}

setJsQuickjsReporterImpl(report);
