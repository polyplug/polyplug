//! Thin binary entry point for the SDK validator.

use std::process::ExitCode;

use sdk_validator::cli::Args;

fn main() -> ExitCode {
    let args: Args = Args::parse_args();

    match sdk_validator::run(args) {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
