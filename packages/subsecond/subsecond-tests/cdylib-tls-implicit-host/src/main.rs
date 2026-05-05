/// Host binary for the cdylib-tls-implicit hot reload test.
///
/// This test verifies that implicit TLS (accessed through a dependency like `rand`,
/// not via explicit `thread_local!`) does not crash after hot patching on any platform.
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
            "libcdylib_tls_implicit.dylib"
        } else if cfg!(target_os = "windows") {
            "cdylib_tls_implicit.dll"
        } else {
            "libcdylib_tls_implicit.so"
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
    let tick: libloading::Symbol<unsafe extern "C" fn()> =
        unsafe { lib.get(b"tick") }.expect("tick not found");

    unsafe { on_load() };

    println!("Implicit TLS host running. Edit tick() prefix to verify patching.");

    loop {
        unsafe { tick() };
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
