//! Thin binary: collect argv and hand off to the library. All testable logic lives in `lib.rs`.

use std::process::ExitCode;

fn main() -> ExitCode {
    herdr_scm::run(std::env::args().collect())
}
