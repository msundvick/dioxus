dioxus_devtools::subsecond::hotpatch_anchor!();

#[no_mangle]
pub extern "C" fn on_load() {
    dioxus_devtools::connect_subsecond();
}

/// Generates a random number using `rand`, which uses thread-local storage internally.
///
/// This tests that implicit TLS (through a dependency, not explicit `thread_local!`)
/// survives hot patching on all platforms.
#[no_mangle]
pub extern "C" fn tick() {
    dioxus_devtools::subsecond::call(|| {
        let n: u32 = rand::random();
        println!("tick v1: random = {n}");
    });
}
