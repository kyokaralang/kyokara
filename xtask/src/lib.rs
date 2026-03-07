pub mod perf;

use std::ffi::OsString;

pub fn run(args: impl IntoIterator<Item = OsString>) -> perf::Result<()> {
    perf::dispatch(args)
}
