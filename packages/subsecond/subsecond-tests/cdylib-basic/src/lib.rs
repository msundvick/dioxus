dioxus_devtools::subsecond::hotpatch_anchor!();

/// Called by the host process after loading the library.
///
/// Connects to the dioxus devserver so that hot patches can be received.
#[no_mangle]
pub extern "C" fn on_load() {
    dioxus_devtools::connect_subsecond();
}

/// The patchable function. Change the return value while `dx serve --lib` is running
/// and observe the output of the host process change without a restart.
#[no_mangle]
pub extern "C" fn get_version() -> u32 {
    dioxus_devtools::subsecond::call(version)
}

fn version() -> u32 {
    let v = vec![1, 23, 3];
    println!("{v:?}");
    13
}
