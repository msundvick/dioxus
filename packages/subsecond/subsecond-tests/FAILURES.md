# Known CI Failures

Tracked failures observed during CI runs, with root-cause analysis.
Tests are expected to continue running; these failures are not masked.

---

## macOS: cdylib tests fail — fat-binary link exports `_main` for cdylib targets (fixed)

**Status: fixed** in `packages/cli/src/build/request.rs` (`run_fat_link`).

**Failing tests:** `cdylib-basic`, `cdylib-tls`, `cdylib-autoconnect`, `cdylib-cxx`

**Symptom:**

```
Failed to generate fat binary: Undefined symbols for architecture arm64:
  "_main", referenced from: <initial-undefines>
ld: symbol(s) not found for architecture arm64
```

**Root cause:** `run_fat_link()` unconditionally passed `-Wl,-exported_symbol,_main`
(Darwin) / `--export-dynamic-symbol,main` (GNU) / `/EXPORT:main` (MSVC) to the linker
so that the subsecond runtime can locate `main` as its ASLR reference point. For
`cdylib` targets there is no `main` function, so the Darwin linker refused with
"undefined symbol". The flags were simply emitted for every target kind.

**Fix:** Guarded the `main` export flags behind `!self.is_cdylib()`. `/HIGHENTROPYVA:NO`
(needed for ASLR overflow prevention on MSVC) is kept unconditionally since it applies
to both binary and cdylib targets.

---

## Windows: all tests fail — dx stack overflow on startup (mitigated in CI)

**Failing tests:** all

**Symptom:**

```
thread 'main' (XXXX) has overflowed its stack
```

The `dx` process exits immediately before producing any output.

**Root cause:** The Windows default main-thread stack is 1 MB (vs 8 MB on Linux/macOS).
Debug builds have larger stack frames due to absent optimizations; `dx` overflows during
startup (cargo-metadata parsing / workspace resolution).

**CI mitigation:** The `Build dx` step in `subsecond-e2e.yml` now passes
`-C link-arg=/STACK:8388608` (8 MB) via `RUSTFLAGS` on Windows only, which embeds a
larger stack size in the PE header.

**Permanent fix:** The dioxus-cli `main()` should re-launch on a thread with an explicit
stack size (e.g. `std::thread::Builder::new().stack_size(8 << 20).spawn(run).unwrap().join()`)
so the binary works correctly regardless of how it's built or linked.

---

## macOS: cdylib tests fail — ASLR sentinel symbol lookup uses wrong number of underscores (fixed)

**Status: fixed** in `packages/cli/src/build/patch.rs` (`aslr_sentinel`).

**Failing tests:** `cdylib-basic`, `cdylib-tls`, `cdylib-autoconnect`, `cdylib-cxx`

**Symptom:**

```
failed to resolve patch symbols: RuntimeError(failed to find ASLR sentinel symbol
'__SUBSECOND_ASLR_REFERENCE' in patch — are debug symbols enabled?
is dioxus_devtools::subsecond::hotpatch_anchor!(); included?)
```

**Root cause:** The Mach-O ABI prefixes every C-linkage symbol with a leading `_`.
`hotpatch_anchor!()` exports `__SUBSECOND_ASLR_REFERENCE` (double underscore, one from
Rust's `#[export_name]` and one from the macro name). Darwin's linker and `object` crate
nlist reader both expose it as `___SUBSECOND_ASLR_REFERENCE` (three underscores). The
`aslr_sentinel()` helper returned the two-underscore form on all platforms, so the
symbol lookup failed on macOS cdylib targets.

**Fix:** `aslr_sentinel()` now returns `"___SUBSECOND_ASLR_REFERENCE"` (three underscores)
when `is_cdylib && triple.operating_system` is Darwin/macOS/iOS. The unit test
`sentinel_cdylib_uses_platform_prefix` was updated to match.

---

## Windows: cdylib tests fail — fat binary link fails with "undefined symbol: main" (under investigation)

**Status: under investigation**

**Failing tests:** `cdylib-basic`, `cdylib-tls`, `cdylib-autoconnect`

**Symptom:**

```
Failed to generate fat binary: rust-lld: error: undefined symbol: main
>>> referenced by ...\msvcrt.lib(exe_main.obj):(int __cdecl invoke_main(void))
```

**What we know:**
- The initial cdylib build succeeds (devserver reaches "Serving your app" state).
- The failure occurs when `run_fat_link()` generates the patch dylib.
- The MSVC CRT startup object `msvcrt.lib(exe_main.obj)` is being pulled in, which only
  happens when the linker is building an EXE (not a DLL). This implies `/DLL` is absent
  from the fat link invocation.
- `run_fat_link()` starts from `rustc_args.link_args` (captured by the rustc wrapper at
  initial build time) and appends a few extra flags. For a cdylib on MSVC, the captured
  args should contain `/DLL` from the original cargo/rustc invocation — but we have not
  verified this empirically.
- The linker invoked is `lld-link` from the Rust toolchain's `gcc-ld/` directory. The
  error message names it as `rust-lld`, which is how that binary identifies itself.
- `run_fat_link()` already guards `/EXPORT:main` behind `!self.is_cdylib()` — so the
  extra export flag is not the direct cause. The root cause is that the link is producing
  an EXE rather than a DLL.

**Open questions:**
1. Does `link_args_file()` (written by the rustc wrapper) actually contain `/DLL` for
   cdylib targets on Windows?
2. Is there code that strips `/DLL` from `args` before the linker is invoked?
3. `thin_link_args()` (used for the hot-patch thin link) hardcodes `/EXPORT:main` at line
   ~2770 without a cdylib guard — is this called during the patch rebuild, and could it
   be causing a separate but related issue?
4. Why does `cdylib-cxx` fail with `rust-lld: error: no input files` while the other
   three get "undefined symbol: main"?

**Needed:** Add tracing/logging to dump the exact linker args captured by the wrapper and
the final args passed to `run_fat_link`, then reproduce on Windows to verify which
assumption is wrong.

---

## Linux / Windows: bin-basic — wrong source vs. test spec (fixed)

**Status: resolved** — test spec updated to match current source (2025-05-03).

The `bin-basic` source was changed from a simple "Hello from bin-basic v1" loop to a
"Closure Struct Layouts" experiment that prints `Captured state2 - x: 10, y: 20 30`.
The e2e test spec now matches the actual source.
