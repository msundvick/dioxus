# Subsecond Hot-Patch Status

## What Works

### Binary targets (`dx serve -p <crate> --hot-patch`)

| Scenario | Status |
|----------|--------|
| Single-crate binary, no TLS | ✅ Working |
| Multi-crate binary (tip + direct deps), no TLS | ✅ Working |
| Transitive deps (dep-of-dep) | ❌ Not yet supported |
| Thread-local storage in patchable code | ❌ Linker error on patch (see below) |

### cdylib targets (`dx serve --lib -p <crate> --hot-patch`)

| Scenario | Status |
|----------|--------|
| Basic function patching | ✅ Working |
| Auto-connect via `#[ctor]` | ✅ Working |
| Thread-local storage in cdylib | ❌ TLS resets on patch (see below) |

---

## Known Issues

### 1. Transitive deps not included in patch (binary)

**Symptom:** Editing a dep-of-a-dep (a crate that isn't a direct dependency of the tip crate) produces no output change after patching. The patch succeeds but the changed code isn't linked.

**Root cause:** The thin-link step only collects cached objects from crates in `modified_crates`. The dependency graph traversal currently stops at direct dependencies of the tip; it doesn't walk the full transitive closure of local path deps.

**Repro:** `dx serve -p bin-transitive-dep --hot-patch`, then edit `bin-dep-nested/src/lib.rs`.

---

### 2. TLS in binary patches causes linker error

**Symptom:**
```
rust-lld: error: relocation R_X86_64_TPOFF32 ... cannot be used with -shared
```

**Root cause:** Thread-locals in a binary are compiled with the `local-exec` TLS model (`R_X86_64_TPOFF32`), which is the fastest model but only valid in executables. The patch is linked as a shared library (`-shared`), which requires a compatible model (`initial-exec` or `general-dynamic`). Rust stable has no `-C tls-model` flag, so we can't change this at compile time without nightly.

**Repro:** `dx serve -p subsecond-tls-harness --hot-patch`, then edit any file containing a `thread_local!`.

**Workaround:** Keep TLS out of patchable closures (`subsecond::call(|| { ... })`). Move thread-locals to a non-patched wrapper function.

---

### 3. TLS resets on cdylib patch

**Symptom:** After a hot-patch, thread-local values inside the cdylib reset to their initial value instead of preserving state.

**Root cause:** Each patch dylib is a new shared object loaded via `dlopen`. Thread-local statics in the patch are re-initialized when the new `.so` is loaded, because they are fresh TLS slots in a new module — they don't alias the originals in the base cdylib.

**Repro:** `dx serve --lib -p cdylib-tls --hot-patch`, then edit the print prefix in `tick()`. Counter resets to 0 after the patch.

**Note:** This is the same fundamental problem as the bin TLS case but manifests differently — instead of a build error, TLS silently reinitializes.

---

## Implementation Notes

### cdylib support additions

- `--lib` flag on `dx serve` / `dx build` selects the cdylib target
- `TargetKind::CDyLib` handling in `request.rs` — cargo args, output path detection
- `.cdylib.json` suffix in `rustcwrapper.rs` for capturing link args separately from bin targets
- `is_cdylib()` / `tip_suffix()` helpers thread through the patch pipeline
- ASLR sentinel: binary patches use `main` (or `__SUBSECOND_ASLR_REFERENCE` macro if present); cdylib patches use a user-exported patchable symbol (since linker demotes injected stubs to local in `.so` files)
- `JumpTable::sentinel` field carries the chosen anchor symbol name to the runtime
- `aslr_reference()` falls back to `RTLD_DEFAULT` when `dlopen(self, RTLD_NOLOAD)` fails (executables on Linux)
- Runner: cdylib builds print the `.so` path + devserver env vars instead of launching a process

### Test harnesses

All in `packages/subsecond/subsecond-tests/`. Run `./run-tests.sh` for automated checks; `./run-tests.sh --e2e` for manual test instructions.

| Crate | Type | Tests |
|-------|------|-------|
| `bin-basic` | bin | Basic single-crate patch |
| `bin-multi-crate` + `bin-dep` | bin | Direct dependency patch |
| `bin-transitive-dep` + `bin-dep-middle` + `bin-dep-nested` | bin | Transitive dep patch (currently failing) |
| `cdylib-basic` + `cdylib-basic-host` | cdylib | Basic function patch |
| `cdylib-tls` + `cdylib-tls-host` | cdylib | TLS preservation (currently failing) |
| `cdylib-autoconnect` + `cdylib-autoconnect-host` | cdylib | Auto-connect via `#[ctor]` |
| `subsecond-tls-harness` (cross-tls-test) | bin | Cross-crate TLS (linker error on patch) |
