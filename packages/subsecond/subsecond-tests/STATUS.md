# Subsecond Hot-Patch Status

## What Works

### Binary targets (`dx serve -p <crate> --hot-patch`)

| Scenario                               | Linux           | macOS | Windows             |
| -------------------------------------- | --------------- | ----- | ------------------- |
| Single-crate binary                    | ✅              | ✅    | ✅                  |
| Multi-crate binary (tip + direct deps) | ✅              | ✅    | ✅                  |
| Transitive deps (dep-of-dep)           | ✅              | ✅    | ✅                  |
| Thread-local storage in patchable code | ❌ Linker error | ✅    | ❌ Runtime Segfault |

### cdylib targets (`dx serve --lib -p <crate> --hot-patch`)

| Scenario                       | Linux         | macOS | Windows             |
| ------------------------------ | ------------- | ----- | ------------------- |
| Basic function patching        | ✅            | ✅    | ✅                  |
| Auto-connect via `#[ctor]`     | ✅            | ✅    | ✅                  |
| Thread-local storage in cdylib | ❌ TLS resets | ✅    | ❌ Runtime Segfault |
| cxx bridge (`cdylib-cxx`)      | ✅            | ✅    | ✅                  |

The Windows cdylib failures are tracked in `FAILURES.md` with diagnostic
instrumentation added to produce ground-truth linker arg dumps in CI.

---

## Known Issues

### 1. TLS in binary patches causes linker error

**Symptom:**

```
rust-lld: error: relocation R_X86_64_TPOFF32 ... cannot be used with -shared
```

**Root cause:** Thread-locals in a binary are compiled with the `local-exec` TLS model
(`R_X86_64_TPOFF32`), which is only valid in executables. The patch is linked as a shared
library (`-shared`), which requires `initial-exec` or `general-dynamic`. Rust stable has
no `-C tls-model` flag, so this can't be changed without nightly.

**Workaround:** Keep TLS out of patchable closures (`subsecond::call(|| { ... })`).

---

### 2. TLS resets on cdylib patch

**Symptom:** After a hot-patch, thread-local values inside the cdylib reset to their
initial value instead of preserving state.

**Root cause:** Each patch dylib is a new shared object loaded via `dlopen`. Thread-local
statics in the patch are re-initialized because they occupy fresh TLS slots in a new
module — they don't alias the originals in the base cdylib.

**Workaround:** Store mutable state in heap-allocated structures (e.g. `Mutex<T>`) rather
than `thread_local!` where hot-patch continuity is required.

---

### 3. Windows cdylib: fat-link fails with "undefined symbol: main" (under investigation)

See `FAILURES.md` for full details. Diagnostic instrumentation is in place; the next CI
run will dump the exact linker args to confirm the root cause.

---

## Implementation Notes

### cdylib support

- `--lib` flag on `dx serve` / `dx build` selects the cdylib target
- `TargetKind::CDyLib` handling in `request.rs` — cargo args, output path detection
- `is_cdylib()` / `tip_suffix()` helpers thread through the patch pipeline
- ASLR sentinel: binary patches use `main`; cdylib patches use `__SUBSECOND_ASLR_REFERENCE`
  exported by `hotpatch_anchor!()`. On macOS, the Mach-O nlist adds a leading `_`, so the
  lookup uses `___SUBSECOND_ASLR_REFERENCE` (three underscores) for Darwin cdylib targets.
- `run_fat_link()` guards the `main`-export linker flag behind `!is_cdylib()` — cdylib
  targets have no `main`, so exporting it causes a linker error.

### CI pipeline

`packages/subsecond/subsecond-tests/e2e.py` is the test runner.
`.github/workflows/subsecond-e2e.yml` runs it on Linux, macOS, and Windows on every push
to `packages/subsecond/**`, `packages/cli/**`, or `packages/dioxus-devtools/**`.

Windows builds `dx` with `-C link-arg=/STACK:8388608` to avoid a stack overflow on the
1 MB default Windows main-thread stack (see `FAILURES.md`).

### Test harnesses

All in `packages/subsecond/subsecond-tests/`.

| Crate                                                      | Type   | Tests                                |
| ---------------------------------------------------------- | ------ | ------------------------------------ |
| `bin-basic`                                                | bin    | Basic single-crate patch             |
| `bin-multi-crate` + `bin-dep`                              | bin    | Direct dependency patch              |
| `bin-transitive-dep` + `bin-dep-middle` + `bin-dep-nested` | bin    | Transitive dep patch                 |
| `cdylib-basic` + `cdylib-basic-host`                       | cdylib | Basic function patch                 |
| `cdylib-tls` + `cdylib-tls-host`                           | cdylib | TLS preservation (resets on patch)   |
| `cdylib-autoconnect` + `cdylib-autoconnect-host`           | cdylib | Auto-connect via `#[ctor]`           |
| `cdylib-cxx` + `cdylib-cxx-host`                           | cdylib | cxx bridge + `compile_as_shared_lib` |
