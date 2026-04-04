/// Host binary for the cdylib-basic hot reload test.
///
/// # Usage
///
/// Terminal 1 — start the devserver targeting the cdylib:
/// ```sh
/// cargo run --package dioxus-cli -- serve --lib --package cdylib-basic --hot-patch
/// ```
///
/// The CLI will print the env vars to set. Export them in Terminal 2:
/// ```sh
/// export DIOXUS_DEVSERVER_IP=127.0.0.1
/// export DIOXUS_DEVSERVER_PORT=8080
/// ```
///
/// Terminal 2 — run this host, pointing it at the library the CLI built:
/// ```sh
/// export CDYLIB_PATH=/path/to/libcdylib_basic.so   # use the path printed by the CLI above
/// cargo run --package cdylib-basic-host
/// ```
///
/// Then edit `cdylib-basic/src/lib.rs` to return `2` from `get_version()`.
/// You should see the printed version change without restarting the host.
fn main() {
    // Resolve the path to the cdylib relative to the workspace target dir.
    // In practice, set CDYLIB_PATH or adjust this to the actual output path.
    let cdylib_path = std::env::var("CDYLIB_PATH").unwrap_or_else(|_| {
        // Default: target/debug in the workspace root
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
            "libcdylib_basic.dylib"
        } else if cfg!(target_os = "windows") {
            "cdylib_basic.dll"
        } else {
            "libcdylib_basic.so"
        };
        workspace
            .join("target/debug")
            .join(lib)
            .to_str()
            .unwrap()
            .to_string()
    });

    // Load the cdylib
    let lib = unsafe { libloading::Library::new(&cdylib_path) }
        .unwrap_or_else(|e| panic!("Failed to load {cdylib_path}: {e}"));

    // Call the init export — this connects to the devserver
    let on_load: libloading::Symbol<unsafe extern "C" fn()> =
        unsafe { lib.get(b"on_load") }.expect("on_load not found");
    unsafe { on_load() };

    // Keep a typed pointer to the patchable function
    let get_version: libloading::Symbol<unsafe extern "C" fn() -> u32> =
        unsafe { lib.get(b"get_version") }.expect("get_version not found");

    println!("Host running. Edit get_version() in cdylib-basic/src/lib.rs to test hot patching.");

    loop {
        let v = unsafe { get_version() };
        println!("version = {v}");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
