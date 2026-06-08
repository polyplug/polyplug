#![no_main]

use core::str::FromStr;

use libfuzzer_sys::fuzz_target;
use polyplug_abi::types::Version;

// Fuzzes `<Version as FromStr>::from_str` with the fuzzed UTF-8 string. Parsing
// must never panic; a clean `Err(ParseVersionError)` is the correct outcome for
// any non-version input.
fuzz_target!(|data: &[u8]| {
    let content: &str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let _: Result<Version, _> = Version::from_str(content);
});
