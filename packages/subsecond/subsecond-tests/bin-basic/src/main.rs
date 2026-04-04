/// Minimal single-crate binary hot-patch test.
///
/// # Usage
///
/// ```sh
/// cargo run --package dioxus-cli -- serve --package bin-basic --hot-patch
/// ```
///
/// Once running, edit the string below and save. The output should change
/// without restarting the process.
fn main() {
    dioxus_devtools::connect_subsecond();
    loop {
        dioxus_devtools::subsecond::call(|| {
            println!("Hello from bin-basic v1");
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
    }
}
