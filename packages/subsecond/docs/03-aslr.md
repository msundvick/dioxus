# ASLR Resolution

## The Problem

The jump table stores compile-time function addresses. But the OS loads both the original binary and the patch shared library at randomized base addresses (ASLR). The compile-time address `0x100001234` becomes `0x7fff_a234_1234` at runtime.

Every address in the jump table map must be adjusted by the runtime slide before it can be used.

## The Anchor Symbol

The `hotpatch_anchor!()` macro exports a stable sentinel symbol:

```rust
// In subsecond/src/lib.rs
macro_rules! hotpatch_anchor {
    () => {
        #[no_mangle]
        #[used]
        pub static __SUBSECOND_ASLR_REFERENCE: u8 = 0;
    }
}
```

This is a single zero byte with a stable, known name. Because its compile-time address is recorded in the `JumpTable` and its runtime address can be looked up via the dynamic linker, it acts as a fixed reference point to compute the slide.

**Binary targets** can use `main` as the sentinel instead (no need to call `hotpatch_anchor!()`).  
**cdylib targets** must call `hotpatch_anchor!()` — there is no `main` symbol to fall back to.

## Platform-Specific Lookup

`aslr_reference()` in [`subsecond/src/lib.rs`](../../subsecond/src/lib.rs) resolves the runtime address of the sentinel:

**Unix (Linux, macOS, Android, iOS):**
```rust
// Try RTLD_NOLOAD first (works for shared libs / cdylib)
let handle = libc::dlopen(ptr::null(), RTLD_NOLOAD | RTLD_GLOBAL);
let sym = libc::dlsym(handle, b"__SUBSECOND_ASLR_REFERENCE\0".as_ptr());
// Fall back to RTLD_DEFAULT (needed for executables on Linux where
// RTLD_NOLOAD returns NULL for the main executable)
```

**Windows:**
```rust
let mut module = HMODULE::default();
GetModuleHandleExW(
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    sentinel_addr as _,
    &mut module,
);
GetProcAddress(module, s!("__SUBSECOND_ASLR_REFERENCE"))
```

**WebAssembly:** Returns `0` — WebAssembly has no ASLR, all addresses are linear memory offsets known at link time.

## macOS: The Triple-Underscore Quirk

Mach-O's ABI prepends a `_` to every C-linkage symbol. The `hotpatch_anchor!()` macro emits `__SUBSECOND_ASLR_REFERENCE` (double underscore in Rust source), which Darwin exposes as `___SUBSECOND_ASLR_REFERENCE` (triple underscore) in the dylib's symbol table.

The CLI's `create_native_jump_table()` in [`packages/cli/src/build/patch.rs`](../../../cli/src/build/patch.rs) accounts for this:
```rust
let sentinel_name = if target.is_darwin() && is_cdylib {
    "___SUBSECOND_ASLR_REFERENCE"
} else {
    "__SUBSECOND_ASLR_REFERENCE"
};
```

## Offset Computation and Address Rewriting

In `apply_patch()` ([`subsecond/src/lib.rs`](../../subsecond/src/lib.rs)):

```rust
// ASLR slide of the running binary
let old_offset = aslr_reference() as i64 - table.aslr_reference as i64;

// ASLR slide of the newly loaded patch library
let new_offset = sentinel_in_patch_lib as i64 - table.new_base_address as i64;

// Rewrite every entry in the map
for (old_addr, new_addr) in table.map.iter_mut() {
    *old_addr = (*old_addr as i64 + old_offset) as u64;
    *new_addr = (*new_addr as i64 + new_offset) as u64;
}
```

After rewriting, the map contains live runtime addresses that can be used directly as function pointers.

## How the CLI Learns the Runtime ASLR Reference

When the app connects to the devserver WebSocket, it includes its current ASLR reference in the URL:

```
ws://localhost:8080/hot-reload?aslr_reference=140234567890&build_id=42&pid=12345
```

The CLI stores this value in `AppBuilder.aslr_reference` and passes it into every subsequent `BuildMode::Thin` build. The build uses this live value when constructing stub addresses for undefined symbols — ensuring the stubs point to the correct runtime addresses in the already-running process.

See [`packages/cli/src/serve/server.rs`](../../../cli/src/serve/server.rs) (`NewConnection` handler) and [`packages/cli/src/build/builder.rs`](../../../cli/src/build/builder.rs).
