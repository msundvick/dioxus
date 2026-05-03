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

## Linux / Windows: bin-basic — wrong source vs. test spec (fixed)

**Status: resolved** — test spec updated to match current source (2025-05-03).

The `bin-basic` source was changed from a simple "Hello from bin-basic v1" loop to a
"Closure Struct Layouts" experiment that prints `Captured state2 - x: 10, y: 20 30`.
The e2e test spec now matches the actual source.
