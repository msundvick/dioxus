fn main() {
    let cdylib_path = std::env::var("CDYLIB_PATH").unwrap_or_else(|_| {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let lib = if cfg!(target_os = "macos") {
            "libcdylib_cxx.dylib"
        } else if cfg!(target_os = "windows") {
            "cdylib_cxx.dll"
        } else {
            "libcdylib_cxx.so"
        };
        workspace
            .join("target/debug")
            .join(lib)
            .to_str()
            .unwrap()
            .to_string()
    });

    let lib = unsafe { libloading::Library::new(&cdylib_path) }
        .unwrap_or_else(|e| panic!("Failed to load {cdylib_path}: {e}"));

    let on_load: libloading::Symbol<unsafe extern "C" fn()> =
        unsafe { lib.get(b"on_load") }.expect("on_load not found");
    unsafe { on_load() };

    let run_compute: libloading::Symbol<unsafe extern "C" fn(i32) -> i32> =
        unsafe { lib.get(b"run_compute") }.expect("run_compute not found");

    let x = 21;
    loop {
        let result = unsafe { run_compute(x) };
        println!("compute({x}) = {result}");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
