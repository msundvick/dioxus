# Patch Generation: Stubs, Object Files, and Jump Tables

Once the Thin build produces `.rcgu.o` incremental object files, the CLI assembles them into a patch shared library that the running app can `dlopen`. The key challenge: these objects reference symbols that exist only in the original binary, not in any `.rlib` or `.dylib` the patch can link against directly.

## Incremental Object Files (`.rcgu.o`)

rustc compiles each crate into one or more codegen units. With incremental compilation enabled, only the units for changed crates are recompiled. These are saved as `.rcgu.o` files (rustc codegen unit object files) alongside the crate's build artifacts.

During the Thin build's NoLink phase, the CLI intercepts the linker invocation and reads the `.rcgu.o` paths from the saved link args by filtering for files ending in `.rcgu.o`. These objects contain the compiled machine code for the changed crate but have unresolved (undefined) symbol references to everything else.

## Symbol Stub Generation

`create_undefined_symbol_stub()` in [`packages/cli/src/build/patch.rs`](../../../cli/src/build/patch.rs) is the bridge between the patch object files and the running binary. For every undefined symbol in the `.rcgu.o` files that can be found in the original binary's symbol table, it synthesizes a stub object containing a tiny piece of machine code that jumps to the symbol's absolute runtime address.

The stub approach avoids having to ship the entire original binary's object files with every patch. Instead, each unknown symbol becomes a trampoline of 8–16 bytes.

### Per-Architecture Stub Machine Code

**x86_64 — Linux/macOS (PIC `jmp [rip+0]`):**
```
FF 25 00 00 00 00   # jmp qword ptr [rip+0]
<8 bytes: absolute address>
```
The `jmp [rip+0]` reads the 8-byte address immediately following the instruction. This is essentially a hand-assembled PLT entry.

**x86_64 — Windows (`movabs + jmp`):**
```
48 B8 <8 bytes: addr>  # movabs rax, <addr>
FF E0                  # jmp rax
```
Windows uses a different convention because the PIC-relative form is not required.

**aarch64 — Linux/macOS/Android/iOS:**
```
58 00 00 00   # ldr x16, [pc, #0]  (load from 8 bytes ahead)
00 02 1F D6   # br x16
<8 bytes: absolute address>
```

**aarch64 — Windows:**
```
MOVZ x16, #<bits 0-15>
MOVK x16, #<bits 16-31>, lsl #16
MOVK x16, #<bits 32-47>, lsl #32
MOVK x16, #<bits 48-63>, lsl #48
BR x16
```
Windows AArch64 doesn't guarantee that the next 8 bytes after a branch instruction are reachable as data, so the address is materialized via immediate instructions instead.

**arm32 (Android armeabi-v7a):**
```
04 F0 1F E5   # ldr pc, [pc, #-4]
<4 bytes: absolute address>
```

### TLS Symbol Stubs

Thread-local symbols (`__thread` / `#[thread_local]`) require special handling. Their TLS block is stored in `.tdata` (initialized) or `.tbss` (zero-initialized) sections, not in regular `.data`.

For each TLS symbol stub, the CLI:
1. Locates the symbol's initialization data in the cached `.tdata` bytes from the Fat binary
2. Creates a new `.tdata` section entry in the stub object with the same initialization data
3. Generates a relocation that wires the new TLS slot to the correct thread-local storage model for the target platform

On macOS, TLS symbol sizes are inferred from adjacent symbol addresses because Mach-O doesn't store explicit sizes in the symbol table.

### Windows `__imp_` Symbols

On Windows, DLL imports are accessed via `__imp_<name>` pointer-sized data entries. For each `__imp_` undefined symbol in the patch object files, the CLI creates a data pointer stub containing the resolved absolute address of the actual function in the running binary.

## The `stub.o` File

All synthesized stubs are written into a single `stub.o` object file and added to the list of objects passed to the real linker. The linker then produces the patch shared library (`.dylib`/`.so`/`.dll`) by combining:

```
[changed .rcgu.o files] + [stub.o] + [thin link args from saved invocation]
```

The old Fat binary is removed from the deps dir before this link step to prevent `dlopen` from returning the already-loaded library from its cache.

## Jump Table Creation

After the patch library is linked, `create_jump_table()` in [`packages/cli/src/build/request.rs`](../../../cli/src/build/request.rs) dispatches to a platform-specific function that compares the original and patch symbol tables by name to build the `old_addr → new_addr` map.

### Unix/macOS: `create_native_jump_table()`

Uses the [`object`](https://crates.io/crates/object) crate to parse ELF (Linux) or Mach-O (macOS) symbol tables from both the original binary and the patch dylib. Matches symbols by name, filters to `STT_FUNC` / `N_SECT` types, and records the compile-time VMA for each match.

### Windows: `create_windows_jump_table()`

Uses the [`pdb`](https://crates.io/crates/pdb) crate to read `.pdb` debug symbol files. PDB stores function name-to-RVA (Relative Virtual Address) mappings. The CLI computes the absolute compile-time address as `image_base + RVA` for both the original and patch, then builds the map from matching names.

### WebAssembly: `create_wasm_jump_table()`

Uses the [`walrus`](https://crates.io/crates/walrus) crate to parse both Wasm modules. Functions are matched through the **indirect function table** (ifunc table) — the Wasm equivalent of a GOT/PLT. Additional handling:
- `GOT.func` and `GOT.mem` imports are satisfied manually
- `env` imports in the patch module are converted to `call_indirect` instructions that dispatch through the ifunc table
- `wasm-bindgen` intrinsics are identified via the `__saved_wbg_` prefix convention
