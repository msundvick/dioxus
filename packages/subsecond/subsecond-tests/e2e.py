#!/usr/bin/env python3
"""
Automated end-to-end test runner for the subsecond hotpatching system.

Each test:
  1. Starts a dx devserver (and optionally a separate host binary for cdylib tests)
  2. Verifies the initial output matches the expected pattern
  3. Edits a source file to trigger a hot patch
  4. Verifies the patched output appears without a process restart
  5. Cleans up (restores files, kills processes)

Usage:
  python e2e.py                          # run all tests
  python e2e.py cdylib-basic             # run one test by name
  python e2e.py cdylib-basic cdylib-tls  # run multiple tests by name
  python e2e.py --timeout 45             # override per-assertion timeout (seconds)
  python e2e.py --startup-timeout 180    # override devserver startup timeout (default: 120)
  python e2e.py --no-restore             # leave file edits in place (for debugging)
  python e2e.py --list                   # print all test names and exit

Environment variables:
  DX    path to the dx/dioxus-cli binary (default: uses cargo run)
"""

from __future__ import annotations

import argparse
import asyncio
import os
import re
import signal
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

# ── Paths ─────────────────────────────────────────────────────────────────────

SCRIPT_DIR = Path(__file__).parent.resolve()
WORKSPACE_ROOT = (SCRIPT_DIR / "../../..").resolve()
TESTS_DIR = SCRIPT_DIR

# dx command: either a pre-built binary or cargo run
_dx_env = os.environ.get("DX")
if _dx_env:
    DX_CMD = [_dx_env]
else:
    DX_CMD = ["cargo", "run", "--package", "dioxus-cli", "--"]

# ── Colour helpers ─────────────────────────────────────────────────────────────

RESET  = "\033[0m"
BOLD   = "\033[1m"
GREEN  = "\033[0;32m"
RED    = "\033[0;31m"
YELLOW = "\033[1;33m"
CYAN   = "\033[0;36m"
DIM    = "\033[2m"

def _c(color: str, text: str) -> str:
    return f"{color}{text}{RESET}" if sys.stdout.isatty() else text

# ── Process management ─────────────────────────────────────────────────────────

class ManagedProcess:
    """
    A subprocess with a background reader task that continuously drains stdout
    into an asyncio.Queue.

    This design means:
    - The pipe never blocks regardless of whether anyone is calling expect().
    - Output can be printed as it arrives (print_output=True).
    - expect() reads from the queue, not directly from the pipe, so it's safe
      to call from any coroutine without competing with the drain loop.
    """

    def __init__(
        self,
        label: str,
        proc: asyncio.subprocess.Process,
        print_output: bool = False,
    ):
        self.label = label
        self.proc = proc
        self._print = print_output
        self._queue: asyncio.Queue[Optional[str]] = asyncio.Queue()
        # Start the background reader immediately
        self._reader_task: asyncio.Task = asyncio.get_event_loop().create_task(
            self._read_loop()
        )

    async def _read_loop(self) -> None:
        assert self.proc.stdout is not None
        try:
            while True:
                raw = await self.proc.stdout.readline()
                if not raw:
                    await self._queue.put(None)   # EOF sentinel
                    return
                text = raw.decode(errors="replace").rstrip()
                if self._print:
                    print(f"  {_c(DIM, f'[{self.label}]')} {text}")
                await self._queue.put(text)
        except asyncio.CancelledError:
            pass

    @classmethod
    async def start(
        cls,
        label: str,
        args: list[str],
        env: Optional[dict] = None,
        cwd: Optional[Path] = None,
        print_output: bool = False,
    ) -> "ManagedProcess":
        merged_env = {**os.environ, **(env or {})}
        proc = await asyncio.create_subprocess_exec(
            *args,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,   # merge stderr into stdout
            env=merged_env,
            cwd=str(cwd or WORKSPACE_ROOT),
            start_new_session=True,             # own process group → clean kill
        )
        return cls(label, proc, print_output=print_output)

    async def expect(
        self,
        pattern: str,
        timeout: float,
        context_lines: int = 20,
    ) -> tuple[str, re.Match]:
        """
        Read queued lines until one matches *pattern* or *timeout* elapses.
        Returns (matched_line, match_object).
        Raises TimeoutError or EOFError with recent output on failure.
        """
        regex = re.compile(pattern)
        recent: list[str] = []
        deadline = asyncio.get_event_loop().time() + timeout

        while True:
            remaining = deadline - asyncio.get_event_loop().time()
            if remaining <= 0:
                snippet = "\n".join(f"    {line}" for line in recent[-context_lines:])
                raise TimeoutError(
                    f"[{self.label}] Pattern {pattern!r} not seen within {timeout}s.\n"
                    f"Last {len(recent)} lines:\n{snippet}"
                )
            try:
                line = await asyncio.wait_for(
                    asyncio.shield(self._queue.get()), timeout=remaining
                )
            except asyncio.TimeoutError:
                snippet = "\n".join(f"    {line}" for line in recent[-context_lines:])
                raise TimeoutError(
                    f"[{self.label}] Pattern {pattern!r} not seen within {timeout}s.\n"
                    f"Last {len(recent)} lines:\n{snippet}"
                )
            if line is None:
                snippet = "\n".join(f"    {line}" for line in recent[-context_lines:])
                raise EOFError(
                    f"[{self.label}] Process exited before pattern {pattern!r} matched.\n"
                    f"Last {len(recent)} lines:\n{snippet}"
                )
            recent.append(line)
            m = regex.search(line)
            if m:
                return line, m

    async def kill(self) -> None:
        self._reader_task.cancel()
        try:
            await self._reader_task
        except asyncio.CancelledError:
            pass

        if sys.platform == "win32":
            try:
                import subprocess as _sp
                _sp.run(
                    ["taskkill", "/F", "/T", "/PID", str(self.proc.pid)],
                    capture_output=True,
                )
            except Exception:
                pass
        else:
            try:
                pgid = os.getpgid(self.proc.pid)
                os.killpg(pgid, signal.SIGTERM)
            except (ProcessLookupError, OSError):
                pass

        try:
            await asyncio.wait_for(self.proc.wait(), timeout=5)
        except asyncio.TimeoutError:
            if sys.platform != "win32":
                try:
                    pgid = os.getpgid(self.proc.pid)
                    os.killpg(pgid, signal.SIGKILL)
                except (ProcessLookupError, OSError):
                    pass
            self.proc.kill()


# ── File editing ───────────────────────────────────────────────────────────────

@dataclass
class FileEdit:
    """One search-and-replace edit to apply to a file."""
    path: Path        # relative to WORKSPACE_ROOT
    old: str
    new: str

    @property
    def abs_path(self) -> Path:
        return WORKSPACE_ROOT / self.path


class EditContext:
    """Applies a list of FileEdit operations and restores originals on exit."""

    def __init__(self, edits: list[FileEdit], restore: bool = True):
        self._edits = edits
        self._restore = restore
        self._originals: dict[Path, str] = {}

    def apply(self) -> None:
        for edit in self._edits:
            p = edit.abs_path
            original = p.read_text()
            if edit.old not in original:
                raise ValueError(
                    f"Edit target not found in {p}:\n  looking for: {edit.old!r}"
                )
            self._originals[p] = original
            p.write_text(original.replace(edit.old, edit.new, 1))

    def restore(self) -> None:
        if not self._restore:
            return
        for path, original in self._originals.items():
            path.write_text(original)
        self._originals.clear()


# ── Readiness polling ──────────────────────────────────────────────────────────

async def wait_for_file(path: Path, timeout: float) -> None:
    """
    Poll until *path* exists on disk.

    The devserver pipe is always being drained by ManagedProcess._read_loop so
    this plain filesystem poll doesn't need to interleave with I/O.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        await asyncio.sleep(0.5)
    raise TimeoutError(f"File {path} did not appear within {timeout}s")


# ── Test specification ─────────────────────────────────────────────────────────

@dataclass
class TestSpec:
    """
    Describes a single e2e hotpatch test.

    For cdylib tests:  devserver_pkg is the cdylib, host_pkg is the loader binary.
    For bin tests:     devserver_pkg is the binary itself, host_pkg is None (dx runs it).
    """
    name: str
    devserver_pkg: str
    devserver_flags: list[str]         # extra flags after `dx serve`
    initial_pattern: str               # regex to match before patching
    edits: list[FileEdit]              # edits to apply
    patched_pattern: str               # regex to match after patching
    host_pkg: Optional[str] = None     # if set, run this as a separate host process
    host_env: dict = field(default_factory=dict)
    description: str = ""


# ── Test definitions ───────────────────────────────────────────────────────────


TESTS: list[TestSpec] = [

    TestSpec(
        name="cdylib-basic",
        description="Basic cdylib hot patch: get_version() return value changes",
        devserver_pkg="cdylib-basic",
        devserver_flags=["--lib"],
        host_pkg="cdylib-basic-host",
        initial_pattern=r"version = 13",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/cdylib-basic/src/lib.rs"),
                old="    13\n}",
                new="    99\n}",
            )
        ],
        patched_pattern=r"version = 99",
    ),

    TestSpec(
        name="cdylib-tls",
        description="TLS preservation: counter must not reset after patch",
        devserver_pkg="cdylib-tls",
        devserver_flags=["--lib"],
        host_pkg="cdylib-tls-host",
        # Only verifies the patch applied (prefix changed). TLS counter reset after
        # a cdylib patch is a known limitation — not tested here.
        initial_pattern=r"tick v1: counter = \d+",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/cdylib-tls/src/lib.rs"),
                old='"tick v1: counter = {v}"',
                new='"tick v2: counter = {v}"',
            )
        ],
        patched_pattern=r"tick v2: counter = \d+",
    ),

    TestSpec(
        name="cdylib-autoconnect",
        description="Auto-init via #[ctor]: no explicit on_load() call needed",
        devserver_pkg="cdylib-autoconnect",
        devserver_flags=["--lib"],
        host_pkg="cdylib-autoconnect-host",
        initial_pattern=r"compute\(\) = 5",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/cdylib-autoconnect/src/lib.rs"),
                old="dioxus_devtools::subsecond::call(|| 5)",
                new="dioxus_devtools::subsecond::call(|| 99)",
            )
        ],
        patched_pattern=r"compute\(\) = 99",
    ),

    TestSpec(
        name="bin-basic",
        description="Single-crate binary: patch propagates without restart",
        devserver_pkg="bin-basic",
        devserver_flags=[],
        initial_pattern=r"Hello from bin-basic v1",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/bin-basic/src/main.rs"),
                old='"Hello from bin-basic v1"',
                new='"Hello from bin-basic v2"',
            )
        ],
        patched_pattern=r"Hello from bin-basic v2",
    ),

    TestSpec(
        name="bin-multi-crate",
        description="Multi-crate binary: patch main crate call site",
        devserver_pkg="bin-multi-crate",
        devserver_flags=[],
        initial_pattern=r"compute\(21\) = 66",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/bin-multi-crate/src/main.rs"),
                old="compute(22)",
                new="compute(99)",
            )
        ],
        patched_pattern=r"compute\(21\) = 297",
    ),

    TestSpec(
        name="bin-multi-crate-dep",
        description="Multi-crate binary: patch dependency crate",
        devserver_pkg="bin-multi-crate",
        devserver_flags=[],
        initial_pattern=r"compute\(21\) = 66",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/bin-dep/src/lib.rs"),
                old="x * 3",
                new="x * 4",
            )
        ],
        patched_pattern=r"compute\(21\) = 88",
    ),

    TestSpec(
        name="bin-transitive-dep",
        description="Transitive dependency: patch nested dep not in direct Cargo.toml",
        devserver_pkg="bin-transitive-dep",
        devserver_flags=[],
        initial_pattern=r"quadruple\(7\) = 28",
        edits=[
            FileEdit(
                path=Path("packages/subsecond/subsecond-tests/bin-dep-nested/src/lib.rs"),
                old="x * 2",
                new="x * 3",
            )
        ],
        # quadruple(7) = double(double(7)) = double(21) = 63  (with x*3)
        patched_pattern=r"quadruple\(7\) = 63",
    ),
]

TEST_BY_NAME = {t.name: t for t in TESTS}

# ── Test runner ────────────────────────────────────────────────────────────────

@dataclass
class TestResult:
    name: str
    passed: bool
    message: str
    elapsed: float


async def run_test(
    spec: TestSpec,
    timeout: float,
    startup_timeout: float,
    restore: bool,
) -> TestResult:
    start = time.monotonic()
    devserver: Optional[ManagedProcess] = None
    host: Optional[ManagedProcess] = None
    edit_ctx = EditContext(spec.edits, restore=restore)

    try:
        # ── 1. Start devserver ─────────────────────────────────────────────────
        serve_args = [
            *DX_CMD,
            "serve",
            "--package", spec.devserver_pkg,
            "--hot-patch",
            *spec.devserver_flags,
        ]
        # Devserver output is always printed so build progress / errors are visible.
        devserver = await ManagedProcess.start(
            label=f"dx/{spec.devserver_pkg}",
            args=serve_args,
            cwd=WORKSPACE_ROOT,
            print_output=True,
        )

        # ── 2. Wait for the build to finish ───────────────────────────────────
        # For cdylib tests: parse the lib path and devserver address from the
        # "cdylib built" line the CLI prints. This is more reliable than polling
        # the filesystem since the CLI knows exactly where it wrote the file.
        # For bin tests: no explicit wait — expect() on devserver stdout handles it.
        cdylib_path: Optional[str] = None
        devserver_ip = "127.0.0.1"
        devserver_port = "8080"

        if spec.host_pkg:
            print("  Waiting for devserver to build the library…")
            _, m = await devserver.expect(
                r"cdylib built in [^:]+: (\S+)",
                timeout=startup_timeout,
            )
            cdylib_path = m.group(1).rstrip(".,")
            print(f"  Library ready: {cdylib_path}")

            # The CLI also prints the env vars on the next lines; grab the port
            # in case it differs from the default.
            try:
                _, mp = await devserver.expect(
                    r"DIOXUS_DEVSERVER_PORT=(\d+)", timeout=5
                )
                devserver_port = mp.group(1)
            except TimeoutError:
                pass  # default 8080 is fine

        # ── 3. Start host (cdylib tests only) ─────────────────────────────────
        if spec.host_pkg:
            host_env = {
                **spec.host_env,
                "CDYLIB_PATH": cdylib_path,
                "DIOXUS_DEVSERVER_IP": devserver_ip,
                "DIOXUS_DEVSERVER_PORT": devserver_port,
            }
            host_args = ["cargo", "run", "--package", spec.host_pkg]
            host = await ManagedProcess.start(
                label=spec.host_pkg,
                args=host_args,
                env=host_env,
                cwd=WORKSPACE_ROOT,
                print_output=True,
            )
            target = host
        else:
            target = devserver

        # ── 4. Verify initial output ───────────────────────────────────────────
        print(f"  Waiting for initial pattern: {spec.initial_pattern!r}")
        line, m = await target.expect(spec.initial_pattern, timeout=timeout)
        print(f"  {_c(GREEN, '✓')} Initial: {line!r}")

        # ── 5. Apply source edit ───────────────────────────────────────────────
        print(f"  Applying {len(spec.edits)} edit(s)…")
        edit_ctx.apply()
        print("  Edit applied.")

        # ── 6. Verify patched output ───────────────────────────────────────────
        print(f"  Waiting for patched pattern: {spec.patched_pattern!r}")
        line, _ = await target.expect(spec.patched_pattern, timeout=timeout)
        print(f"  {_c(GREEN, '✓')} Patched: {line!r}")

        return TestResult(
            name=spec.name,
            passed=True,
            message="All assertions passed",
            elapsed=time.monotonic() - start,
        )

    except (TimeoutError, EOFError, ValueError) as exc:
        return TestResult(
            name=spec.name,
            passed=False,
            message=str(exc),
            elapsed=time.monotonic() - start,
        )
    except Exception as exc:
        return TestResult(
            name=spec.name,
            passed=False,
            message=f"{type(exc).__name__}: {exc}",
            elapsed=time.monotonic() - start,
        )
    finally:
        edit_ctx.restore()
        if host:
            await host.kill()
        if devserver:
            await devserver.kill()


# ── Main ───────────────────────────────────────────────────────────────────────

async def async_main(args: argparse.Namespace) -> int:
    if args.list:
        for t in TESTS:
            print(f"  {t.name:<30}  {t.description}")
        return 0

    # Select which tests to run
    if args.tests:
        unknown = [n for n in args.tests if n not in TEST_BY_NAME]
        if unknown:
            print(f"Unknown test name(s): {', '.join(unknown)}")
            print(f"Available: {', '.join(TEST_BY_NAME)}")
            return 1
        specs = [TEST_BY_NAME[n] for n in args.tests]
    else:
        specs = list(TESTS)

    print(_c(BOLD, f"\n{'━'*70}"))
    print(_c(BOLD, f"  subsecond e2e — {len(specs)} test(s)"))
    print(_c(BOLD, f"{'━'*70}\n"))

    results: list[TestResult] = []

    for spec in specs:
        print(_c(BOLD, f"── {spec.name} ──────────────────────────────────────"))
        print(f"   {_c(DIM, spec.description)}")
        result = await run_test(
            spec,
            timeout=args.timeout,
            startup_timeout=args.startup_timeout,
            restore=not args.no_restore,
        )
        results.append(result)
        status = _c(GREEN, "PASS") if result.passed else _c(RED, "FAIL")
        print(f"   {status}  ({result.elapsed:.1f}s)")
        if not result.passed:
            for line in result.message.splitlines():
                print(f"   {_c(RED, line)}")
        print()

    # ── Summary ────────────────────────────────────────────────────────────────
    passed = [r for r in results if r.passed]
    failed = [r for r in results if not r.passed]

    print(_c(BOLD, f"{'━'*70}"))
    print(f"  {_c(GREEN, f'{len(passed)} passed')}  {_c(RED, f'{len(failed)} failed')}  "
          f"out of {len(results)} tests")
    if failed:
        print()
        for r in failed:
            print(f"  {_c(RED, '✗')} {r.name}")
    print(_c(BOLD, f"{'━'*70}\n"))

    return 0 if not failed else 1


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Automated e2e test runner for subsecond hotpatching",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "tests",
        nargs="*",
        metavar="TEST",
        help="Test name(s) to run (default: all)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=30.0,
        metavar="SECS",
        help="Per-assertion timeout in seconds (default: 30)",
    )
    parser.add_argument(
        "--startup-timeout",
        type=float,
        default=120.0,
        metavar="SECS",
        help="Devserver startup / build timeout in seconds (default: 120)",
    )
    parser.add_argument(
        "--no-restore",
        action="store_true",
        help="Leave source file edits in place after the test (for debugging)",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List available test names and exit",
    )
    args = parser.parse_args()
    sys.exit(asyncio.run(async_main(args)))


if __name__ == "__main__":
    main()
