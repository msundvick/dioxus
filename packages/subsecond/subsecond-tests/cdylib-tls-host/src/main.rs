/// Host binary for the cdylib-tls hot reload test.
///
/// This test verifies that thread-local storage (TLS) inside a cdylib survives hot patching.
/// If TLS re-initialization is broken, the counter printed by `tick()` will reset to 0 or 1
/// after a patch. The correct behaviour is for the counter to keep incrementing.
///
/// # Usage
///
/// Terminal 1:
/// ```sh
/// cargo run --package dioxus-cli -- serve --lib --package cdylib-tls --hot-patch
/// ```
/// The CLI prints the path to the built library. Export it along with the devserver env vars,
/// then in Terminal 2:
/// ```sh
/// export CDYLIB_PATH=<path printed by CLI>
/// export DIOXUS_DEVSERVER_IP=127.0.0.1
/// export DIOXUS_DEVSERVER_PORT=8080
/// cargo run --package cdylib-tls-host
/// ```
///
/// Once the counter is above ~5, edit `tick()` to print `"tick v2: counter = {v}"`.
/// After the patch applies, the counter should continue from where it was, not reset.
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
            "libcdylib_tls.dylib"
        } else if cfg!(target_os = "windows") {
            "cdylib_tls.dll"
        } else {
            "libcdylib_tls.so"
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

    println!(
        "TLS host running. Edit tick() prefix to verify patching. Counter must NOT reset after patch."
    );

    loop {
        unsafe { tick() };
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
