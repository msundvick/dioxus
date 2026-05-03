dioxus_devtools::subsecond::hotpatch_anchor!();

#[cxx::bridge(namespace = "cdylib_cxx")]
mod ffi {
    extern "Rust" {
        fn compute(x: i32) -> i32;
    }
}

#[no_mangle]
pub extern "C" fn on_load() {
    dioxus_devtools::connect_subsecond();
}

/// C-ABI entry point called by the host via libloading.
/// Routes through the cxx-bridged compute() to exercise compile_as_shared_lib
/// symbol export alongside subsecond hot-reload.
#[no_mangle]
pub extern "C" fn run_compute(x: i32) -> i32 {
    compute(x)
}

fn compute(x: i32) -> i32 {
    dioxus_devtools::subsecond::call(|| x * 2)
}
