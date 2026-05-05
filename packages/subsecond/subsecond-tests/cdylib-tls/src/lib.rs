dioxus_devtools::subsecond::hotpatch_anchor!();

use std::cell::Cell;

thread_local! {
    /// A counter that lives in thread-local storage inside the cdylib.
    ///
    /// After a hot patch, the counter must keep incrementing from where it left off —
    /// if TLS re-initialization happens incorrectly the counter will reset to 0,
    /// which is the failure mode this test detects.
    static COUNTER: Cell<u32> = const { Cell::new(0) };
}

#[no_mangle]
pub extern "C" fn on_load() {
    dioxus_devtools::connect_subsecond();
}

/// Increments the TLS counter and prints it.
///
/// Change the prefix string (e.g. "tick v1" → "tick v2") to verify that the function
/// was patched. The counter value must continue increasing, not reset to 0 or 1.
#[no_mangle]
pub extern "C" fn tick() {
    println!("[tick] entered tick()");
    dioxus_devtools::subsecond::call(|| {
        println!("[tick] inside call closure, before COUNTER.with");
        COUNTER.with(|c| {
            println!("[tick] inside COUNTER.with");
            let v = c.get() + 3;
            c.set(v);
            println!("tick v1: counter = {v}");
        });
    });
    println!("[tick] tick() returning");
}
