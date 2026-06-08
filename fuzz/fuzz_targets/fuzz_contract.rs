#![no_main]

use libfuzzer_sys::fuzz_target;
use polyplugc::parser::{parse_api_str, parse_bundle_str};

// Fuzzes both contract `.toml` parsers (`parse_api_str` and `parse_bundle_str`)
// with the same fuzzed UTF-8 string. Both must reject malformed input with a
// clean `Err` rather than panic; the result is discarded and libFuzzer catches
// any panic/UB.
fuzz_target!(|data: &[u8]| {
    let content: &str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let _: Result<_, _> = parse_api_str(content);
    let _: Result<_, _> = parse_bundle_str(content);
});
