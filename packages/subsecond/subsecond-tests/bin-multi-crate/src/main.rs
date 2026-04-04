/// Multi-crate binary hot-patch test. Verifies that patching works when the
/// changed code calls into a dependency crate. No TLS.
///
/// # Usage
///
/// ```sh
/// cargo run --package dioxus-cli -- serve --package bin-multi-crate --hot-patch
/// ```
///
/// Once running, edit the argument to `compute()` or edit `bin-dep/src/lib.rs`
/// and save. The output should change without restarting the process.
use bin_dep::compute;

fn main() {
    dioxus_devtools::connect_subsecond();
    loop {
        dioxus_devtools::subsecond::call(|| {
            println!("compute(21) = {}", compute(22));
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
    }
}
