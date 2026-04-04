/// Host binary for the cdylib-autoconnect hot reload test.
///
/// This test verifies that a cdylib can connect to the devserver automatically via a
/// `#[ctor]` library constructor, without requiring the host to call any init function.
/// Simply loading the library is sufficient.
///
/// # Usage
///
/// Terminal 1:
/// ```sh
/// cargo run --package dioxus-cli -- serve --lib --package cdylib-autoconnect --hot-patch
/// ```
/// The CLI prints the path to the built library. Export it along with the devserver env vars,
/// then in Terminal 2:
/// ```sh
/// export CDYLIB_PATH=<path printed by CLI>
/// export DIOXUS_DEVSERVER_IP=127.0.0.1
/// export DIOXUS_DEVSERVER_PORT=8080
/// cargo run --package cdylib-autoconnect-host
/// ```
///
/// Edit `compute()` to return a different value (e.g. `99`). The output should change
/// without restarting the host, and without the host calling any init function explicitly.
fn main() {
    let cdylib_path = std::env::var("CDYLIB_PATH").unwrap_or_else(|_| {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest.parent().unwrap().parent().unwrap().parent().unwrap().parent().unwrap();
        let lib = if cfg!(target_os = "macos") {
            "libcdylib_autoconnect.dylib"
        } else if cfg!(target_os = "windows") {
            "cdylib_autoconnect.dll"
        } else {
            "libcdylib_autoconnect.so"
        };
        workspace.join("target/debug").join(lib).to_str().unwrap().to_string()
    });

    // Loading the library triggers the #[ctor] init, which calls connect_subsecond().
    // No explicit on_load() call is needed.
    let lib = unsafe { libloading::Library::new(&cdylib_path) }
        .unwrap_or_else(|e| panic!("Failed to load {cdylib_path}: {e}"));

    let compute: libloading::Symbol<unsafe extern "C" fn() -> i32> =
        unsafe { lib.get(b"compute") }.expect("compute not found");

    println!("Autoconnect host running (no explicit init call). Edit compute() to test hot patching.");

    loop {
        subsecond::call(|| {
            let v = unsafe { compute() };
            println!("compute() = {v}");
        });
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
