//! Executable entry point for the `devscout` command-line interface.

fn main() {
    devscout_rs::cli::dispatch(std::env::args().collect());
}
