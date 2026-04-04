/// Transitive-dependency binary hot-patch test.
///
/// Dependency chain: bin-transitive-dep → bin-dep-middle → bin-dep-nested
/// bin-dep-nested is NOT a direct dependency of this crate.
///
/// # Usage
///
/// ```sh
/// cargo run --package dioxus-cli -- serve --package bin-transitive-dep --hot-patch
/// ```
///
/// Patch options (each should update output without restart):
/// - Edit the argument to `quadruple()` in this file.
/// - Edit `bin-dep-middle/src/lib.rs` (direct dep).
/// - Edit `bin-dep-nested/src/lib.rs` (transitive dep — not listed in this Cargo.toml).
use bin_dep_middle::quadruple;

fn main() {
    dioxus_devtools::connect_subsecond();
    loop {
        dioxus_devtools::subsecond::call(|| {
            println!("quadruple(7) = {}", quadruple(7));
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
    }
}
