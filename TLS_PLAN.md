Here is the complete, end-to-end architectural plan to implement the cdylib compiler trick and the TLS memory interception.

This plan is broken into **three distinct phases**. I have included specific debugging checkpoints for each phase so that if a piece of the pipeline fails, you will know exactly which component to investigate.

---

### Phase 1: Force `rustc` to emit Global Dynamic TLS

**Goal:** Modify the Dioxus CLI so that when it compiles the patch artifact for a binary target, it forces the compiler to use `cdylib` constraints, eradicating the `R_X86_64_TPOFF32` linker error.

**Where to modify:** `packages/cli/src/build/request.rs` (or wherever the `cargo` command for the hotpatch artifact is constructed).

**Action:**
Instead of allowing Cargo to run a default `build` for the binary, construct a `cargo rustc` command and append the raw flag.

```rust
// Replace the standard cargo build command for the patch with this:
let mut cmd = std::process::Command::new("cargo");
cmd.arg("rustc");
cmd.arg("--bin").arg(&request.main_target); // Target the user's binary

// ... inject existing target, profile, and feature args ...

// The Magic: Pass the raw flag to rustc to force GD TLS
cmd.arg("--");
cmd.arg("--crate-type").arg("cdylib");
```

**✅ Checkpoint 1 (Verification):**

1. Run `cargo run -p dioxus-cli -- serve -p subsecond-tls-harness --hot-patch`.
2. Look at the CLI output. The `rust-lld: error: relocation R_X86_64_TPOFF32` error should completely disappear.
3. **Debug if it fails:** If the error persists, Dioxus's custom `rustc` wrapper might be stripping the `--crate-type` flag. You will need to inspect the `workspace_rustc_args` mapping in the CLI to ensure `"cdylib"` is explicitly appended to the `args` array for the tip crate.

---

### Phase 2: Fix the Phantom Symbol Trap in the CLI

**Goal:** Ensure the jump table perfectly resolves `"main"` for binary targets by ignoring dead-stripped phantom symbols (`0` addresses) left behind by the cdylib compilation.

**Where to modify:** `packages/cli/src/build/hotpatch.rs` (inside `create_native_jump_table`).

**Action:**
Apply the `.filter(|&&addr| addr != 0)` to the symbol lookups to bypass the `0 < 1852a0` validation crash.

```rust
    let os_main = match triple.operating_system {
        OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) | OperatingSystem::IOS(_) => "_main",
        _ => "main",
    };

    let new_base_address = new_name_to_addr
        .get("__SUBSECOND_ASLR_REFERENCE")
        .filter(|&&addr| addr != 0) // IGNORE PHANTOM SYMBOLS
        .or_else(|| new_name_to_addr.get(os_main).filter(|&&addr| addr != 0))
        .cloned()
        .context("failed to find valid anchor or main in patch")?;

    let aslr_reference = old_name_to_addr
        .get("__SUBSECOND_ASLR_REFERENCE")
        .map(|s| s.address)
        .filter(|&addr| addr != 0) // IGNORE PHANTOM SYMBOLS
        .or_else(|| {
            old_name_to_addr
                .get(os_main)
                .map(|s| s.address)
                .filter(|&addr| addr != 0)
        })
        .context("failed to find valid anchor or main in host")?;
```

**✅ Checkpoint 2 (Verification):**

1. Save the file and trigger a hotpatch for Test 4.
2. The CLI should successfully print `Successfully mapped functions: X` and generate the `.so` patch file.
3. **Debug if it fails:** If it panics at `.context("failed to find...")`, it means the `cdylib` compilation _completely_ stripped `"main"` (not just a 0-address). If this happens, we must fall back to injecting the `hotpatch_anchor!()` macro into Test 4 to give it a guaranteed symbol.

---

### Phase 3: The `__tls_get_addr` GOT Hook (Linux)

**Goal:** Intercept the dynamic TLS lookup inside the patch `.so` so it queries the base executable's Thread Local memory instead of its own empty block.

**Where to modify:** `packages/subsecond/subsecond/src/lib.rs` (inside `apply_patch`).

**Action 1: Define the Hook**
Add this custom trampoline to the top of `subsecond/src/lib.rs`. This intercepts the module ID.

```rust
#[cfg(target_os = "linux")]
#[repr(C)]
pub struct tls_index {
    pub ti_module: usize,
    pub ti_offset: usize,
}

#[cfg(target_os = "linux")]
#[no_mangle]
pub unsafe extern "C" fn subsecond_tls_get_addr(ti: *mut tls_index) -> *mut u8 {
    // 1 is almost always the main executable module ID in Linux ELF
    (*ti).ti_module = 1;

    // Retrieve the actual libc implementation
    let real_tls: unsafe extern "C" fn(*mut tls_index) -> *mut u8 =
        std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, c"__tls_get_addr".as_ptr() as _));

    real_tls(ti)
}
```

**Action 2: Overwrite the GOT**
Inside `apply_patch`, immediately after `libloading::Library::new(&table.lib)`, you must locate the `.got.plt` of the newly loaded library and overwrite the `__tls_get_addr` pointer.

_(Note: Parsing ELF GOT tables natively in Rust requires the `elf` or `goblin` crate. I recommend adding `goblin` to `subsecond` dependencies for safety)._

```rust
#[cfg(target_os = "linux")]
unsafe {
    // 1. Calculate the base address of the loaded patch.so in memory
    let patch_base = aslr_reference() - table.aslr_reference as usize + table.new_base_address as usize;

    // 2. Parse the ELF headers (using goblin or manual parsing) to find DT_JMPREL / .rel.plt
    // 3. Find the relocation entry specifically for "__tls_get_addr"
    // 4. Calculate its exact memory address: let got_entry_ptr = patch_base + relocation_offset;

    // 5. Unprotect the memory page so we can write to it
    let page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
    let page_start = (got_entry_ptr as usize) & !(page_size - 1);
    libc::mprotect(page_start as *mut libc::c_void, page_size, libc::PROT_READ | libc::PROT_WRITE);

    // 6. Overwrite the GOT pointer with our custom hook!
    std::ptr::write(got_entry_ptr as *mut usize, subsecond_tls_get_addr as usize);

    // 7. Reprotect the memory
    libc::mprotect(page_start as *mut libc::c_void, page_size, libc::PROT_READ | libc::PROT_EXEC);
}
```

**✅ Checkpoint 3 (Verification):**

1. Run Test 4 (`subsecond-tls-harness`).
2. Trigger the hotpatch.
3. The host should not only successfully detour into the patch, but the TLS state variables (like `counter`) should persist precisely from where they left off!
4. **Debug if it fails:** If it segfaults immediately upon patch execution, `(*ti).ti_module = 1;` might be incorrect for your specific Linux distro. Print the `ti_module` value being passed to the hook _before_ overwriting it to see what ID the patch was originally assigned, then verify via `/proc/self/maps` that module `1` is indeed the host binary.
