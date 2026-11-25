//! The helper side of the askpass bridge as git starts it, for the tests: the
//! app does the same in its own `main`.
use ncrs_git::askpass::run_if_asked;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(run_if_asked(&args).unwrap_or(2));
}
