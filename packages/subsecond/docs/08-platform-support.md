# Platform Support and Known Issues

## Support Matrix

| Scenario | Linux | macOS | Windows | Android | iOS (sim) | Wasm |
|----------|-------|-------|---------|---------|-----------|------|
| Single-crate binary | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multi-crate binary | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| cdylib basic patching | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| TLS in binary patch | ❌ | ✅ | ❌ | ❌ | ❌ | N/A |
| TLS in cdylib patch | ❌ | ✅ | ❌ | ❌ | ❌ | N/A |

CI runs on Linux, macOS, and Windows via `.github/workflows/subsecond-e2e.yml`. Android, iOS, and Wasm are tested manually.

---

## Issue 1: TLS in Binary Patches — Linux, Windows, Android

**Symptom (Linux):**
```
rust-lld: error: relocation R_X86_64_TPOFF32 ... cannot be used with -shared
```

**Root cause:** `thread_local!` statics in a binary crate are compiled with the `local-exec` TLS model (`R_X86_64_TPOFF32` / `R_AARCH64_TLSLE_*`). This model assumes the TLS block is at a fixed offset from the thread pointer, valid only in executables. The patch is linked as a shared library (`-shared` / `-dylib`), which requires `initial-exec` or `general-dynamic` TLS models. Rust stable has no `-C tls-model` flag, so this cannot be changed without nightly.

**Workaround:** Keep `thread_local!` statics out of patchable closures (`subsecond::call(|| { ... })`). Move mutable per-thread state to heap-allocated structures (e.g., `RefCell<T>` in a `Rc` or `Mutex<T>`).

**Potential fix (not yet implemented):** A nightly build could add `-C tls-model=initial-exec` to the Thin build rustc invocation. On stable, this is not possible without forking rustc.

---

## Issue 2: TLS Resets on cdylib Patch — Linux

**Symptom:** After a hot-patch, `thread_local!` values in the cdylib reset to their initial value.

**Root cause:** Each patch is a new shared object loaded via `dlopen`. Thread-local statics occupy **fresh TLS slots** in the new module — they do not alias the originals in the base cdylib. The first time patched code reads a `thread_local!`, it gets the initial value, not whatever was accumulated before the patch.

**macOS:** Not affected. macOS uses a different TLS ABI (`tlv_get_addr` thunk) which goes through an indirection that the linker can alias correctly.

**Workaround:** Store state that must survive patches in heap-allocated structures. `thread_local! { static FOO: RefCell<State> }` → use `Arc<Mutex<State>>` in a regular static instead.

---

## Issue 3: TLS Crash on Windows — cdylib and Binary Patches

**Symptom:** `STATUS_ACCESS_VIOLATION` (segfault) when patched code accesses a `thread_local!`.

**Root cause:** Windows fires `DLL_THREAD_ATTACH` (which initializes TLS for a DLL) only for threads created *after* the DLL is loaded via `LoadLibrary`. The patch library is loaded on the WebSocket/devserver thread. The main application thread — which existed before the patch was loaded — never has its TLS initialized for the new patch DLL. The first access to a `thread_local!` in the patch on the main thread dereferences a NULL pointer.

**Scope:** Only affects `thread_local!` declared directly in the hot-patchable (tip) crate. TLS in rlib or cdylib *dependencies* is safe because those symbols are accessed through stubs pointing into the original library, where TLS was initialized correctly at startup.

**Potential fixes (not yet implemented):**
1. Walk all existing threads via `CreateToolhelp32Snapshot` / `Thread32First` and manually invoke TLS initializers for the new DLL on each thread (requires a custom `DllMain`).
2. Replace `thread_local!` usage in subsecond's own infrastructure with `TlsAlloc` / `TlsGetValue` explicit TLS, decoupling initialization from `DLL_THREAD_ATTACH`.

**Workaround:** Same as Issue 2 — use `Arc<Mutex<T>>` or similar heap-allocated shared state instead of `thread_local!` in hot-patchable cdylib crates on Windows.

---

## Issue 4: Windows Stack Overflow on `dx` Startup (Debug Builds)

**Symptom:** `dx serve` crashes on startup on Windows when built in debug mode.

**Root cause:** Windows default main-thread stack is 1 MB vs. 8 MB on Linux/macOS. Debug builds have larger stack frames; `dx`'s startup path (cargo-metadata parsing, workspace resolution) overflows the stack.

**CI mitigation:** `dx` is built with `RUSTFLAGS=-C link-arg=/STACK:8388608` to embed an 8 MB stack size in the PE header.

**Permanent fix (not yet applied):** Spawn `main()`'s body on an explicit-stack-size thread:
```rust
fn main() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
```

---

## Android: MTE Pointer Tagging

Android on ARMv8.5+ devices uses the Memory Tagging Extension (MTE), which stores a 4-bit tag in the top byte of pointers. Jump table lookups use the raw integer value of a function pointer as the map key. Without stripping the tag, a tagged pointer would not match its untagged compile-time address in the map.

`call_as_ptr()` strips the MTE tag before the lookup and (if needed) re-applies it after resolving the new address:

```rust
// Strip top byte (tag) on Android
#[cfg(target_os = "android")]
let ptr = ptr & 0x00FF_FFFF_FFFF_FFFF;
```

---

## iOS: Code Signing Restriction

iOS devices require all executable code to be signed. Loading a dynamically compiled patch library via `dlopen` at runtime is not possible on physical iOS devices without a developer entitlement that Apple does not grant for App Store apps.

**iOS Simulator** is not subject to this restriction and is fully supported. Hot-patching works on the simulator using the same code path as macOS.

---

## macOS: Darwin Symbol Prefix

Mach-O's C ABI prepends `_` to every symbol name. The `hotpatch_anchor!()` macro emits `__SUBSECOND_ASLR_REFERENCE` (double underscore in Rust), which Darwin stores as `___SUBSECOND_ASLR_REFERENCE` (triple underscore) in the dylib symbol table. The CLI accounts for this explicitly when looking up the sentinel on Darwin cdylib targets.

See [03-aslr.md](03-aslr.md) for details.

---

## Previously Fixed Issues

**macOS cdylib: `_main` export caused linker error**  
Fixed by guarding the `run_fat_link()` main-export flag behind `!is_cdylib()`. cdylib targets have no `main`; Darwin's linker rejected the unconditional `-Wl,-exported_symbol,_main`.

**macOS cdylib: dangling `.dylib` symlink from `cxx` flags**  
The `cxx` build script emits `-Wl,-install_name,@rpath/libcdylib_cxx.dylib` as a linker flag. The CLI was incorrectly treating this flag string as a file path (because it ends in `.dylib`) and creating a dangling symlink. Fixed by requiring the arg to be an absolute path (`PathBuf::from(arg).is_absolute()`) before treating it as a dylib file.
