/// Library constructor — runs automatically when the cdylib is loaded by any host process.
///
/// This means the host does NOT need to call an explicit `on_load()` function. Just loading
/// the library (via `libloading::Library::new`) is sufficient to connect to the devserver.
#[ctor::ctor]
fn init() {
    dioxus_devtools::connect_subsecond();
}

/// Patchable function. Change the return value to verify hot patching works without
/// any explicit init call from the host side.
#[no_mangle]
pub extern "C" fn compute() -> i32 {
    42
}
