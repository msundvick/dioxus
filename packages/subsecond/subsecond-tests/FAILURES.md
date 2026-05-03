# Known CI Failures

Tracked failures observed during CI runs, with root-cause analysis.
Tests are expected to continue running; these failures are not masked.

---

## macOS: cdylib tests fail — CLI attempts fat-binary link for `--lib` builds

**Failing tests:** `cdylib-basic`, `cdylib-tls`, `cdylib-autoconnect`, `cdylib-cxx`

**Symptom:**
```
Failed to generate fat binary: Undefined symbols for architecture arm64:
  "_main", referenced from:
      <initial-undefines>
ld: symbol(s) not found for architecture arm64
clang: error: linker command failed with exit code 1
Build failed: No such file or directory (os error 2)
```

**Root cause:** When `dx serve --lib --hot-patch` is invoked on macOS (arm64), the CLI
invokes `lipo` (or equivalent) to produce a universal/"fat" binary as a post-link step.
This step expects a `_main` symbol, which a `cdylib` intentionally does not have.
The post-link fat-binary step should be skipped entirely for `TargetKind::CDyLib`.

**Location:** dioxus-cli build pipeline, somewhere in the post-link/`lipo` path for macOS.

**Workaround:** None at the test level. Requires a fix in the CLI to gate the fat-binary
step on target kind.

**Platform:** macOS arm64 (CI: `macos-latest`). Linux and Windows unaffected.

---

## Linux / Windows: bin-basic — wrong source vs. test spec (fixed)

**Status: resolved** — test spec updated to match current source (2025-05-03).

The `bin-basic` source was changed from a simple "Hello from bin-basic v1" loop to a
"Closure Struct Layouts" experiment that prints `Captured state2 - x: 10, y: 20 30`.
The e2e test spec now matches the actual source.
