# Known Limitations

These are architectural constraints in subsecond's current design, not bugs. Each entry describes what doesn't work, why, and what workarounds or potential fixes exist.

---

## Struct Layout Changes Crash

**What:** Changing struct fields, field order, or alignment while the app is running will crash if new patched functions are called with state that was allocated under the old layout.

**Why:** The jump table replaces function pointers but does not migrate heap-allocated data. A new `get_x()` that reads field offset 16 will crash if the existing `Foo` value in memory only has 8 bytes.

**Framework responsibility:** Frameworks that use subsecond (like Dioxus) must detect layout-breaking changes and discard all existing state, forcing a full re-instance from scratch. Dioxus does this by throwing out the entire virtual DOM on each patch.

**No planned fix:** Migrating arbitrary heap-allocated Rust structs at runtime is not tractable in the general case.

---

## Statics Are Not Destructed

**What:** New global statics (`static FOO: T = ...`) can be added via patches and will be initialized. However, their destructors will **never run** — neither when the next patch is applied nor when the process exits.

**Why:** The patch library is intentionally leaked (see [06-runtime-apply.md](06-runtime-apply.md)). Leaked libraries cannot fire their `atexit` / static-destructor chains.

**Renamed statics** are treated as new statics — the old value is abandoned in place, and a fresh one is initialized. This can cause resource leaks if the static holds OS resources (file handles, sockets).

---

## `ptr_address` Always Returns Current Version

**What:** `subsecond::ptr_address(f)` returns the _current_ address of function `f` — the new version if a patch has been applied.

**Why:** There is no per-function "has this function changed since the last call?" signal. Every call to `ptr_address` reflects the current jump table state.

**Effect on frameworks:** A framework using `ptr_address` to detect whether a component function changed (for memoization purposes) will consider every function "changed" on every patch, potentially re-rendering all components even if only one changed. This is conservative but correct.

---

## `FnOnce` Not Supported

**What:** `subsecond::call` only accepts `FnMut`. `FnOnce` closures (those that consume captured values) cannot be hot-patched.

**Why:** The retry loop in `call()` requires the closure to be callable multiple times. A `FnOnce` that consumed its captures on the first call cannot be re-invoked after a `HotFnPanic`.

**Workaround:** Restructure `FnOnce` logic to take ownership of values inside the closure rather than in captures, or wrap the owned value in an `Option` and take it on first call.

---

## Maximum 9 Function Arguments

**What:** The `HotFunction` trait is implemented for `FnMut` closures with 0 through 9 arguments. Functions with 10+ arguments are not hot-patchable.

**Why:** The `impl_hot_function!` macro generates implementations for arities 0–9. This covers virtually all real-world use cases, but the limit is arbitrary and could be raised.

**Workaround:** Bundle arguments into a struct.

---

## WebAssembly: No Panic Unwinding

**What:** The `HotFnPanic` retry mechanism is disabled on `wasm32`. If a patch arrives mid-call, panics from stale inner calls are not caught and retried at the `call()` boundary.

**Why:** WebAssembly (in the widely supported MVP and bulk-memory feature sets) does not support stack unwinding. `std::panic::catch_unwind` is a no-op on `wasm32-unknown-unknown`.

**Framework responsibility:** Framework authors on Wasm must manually drop futures or tasks that hold references to functions being replaced, before triggering re-execution.

---

## Implicit TLS in Dependencies

**What:** `thread_local!` statics in _dependency_ crates (rlibs) that are linked into the patchable tip crate may or may not survive a patch correctly.

**Why:** Whether a dep's TLS symbol is included in the patch dylib or resolved via a stub depends on whether the dep crate was recompiled. If the TLS symbol is stubbed (pointing into the original binary), its value persists. If it was recompiled into the patch, it gets a fresh slot.

**Implicit TLS from crates like `rand`** is generally safe because `rand`'s TLS symbols live in `rand`'s own rlib and are resolved via stubs — they point to the already-initialized slots in the original binary.

**Explicit TLS in the tip crate** is the dangerous case (see [08-platform-support.md](08-platform-support.md) Issues 1–3).
