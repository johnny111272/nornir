#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir dispatcher tools (batch processing binaries).

Single command: ./tools/nornir/deploy_dispatchers.py

Builds and deploys:
  - Dispatcher binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all dispatcher commands work immediately.
Verify with: {binary_name} --help
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

DISPATCHER_CRATES = [
    "split_jsonl_batches",
]


def build_dispatchers() -> bool:
    """Build dispatcher tool binaries."""
    packages = []
    for crate in DISPATCHER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        sys.stderr.write("FAIL: cargo build (dispatchers)\n")
        sys.exit(1)
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for dispatcher tools."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in DISPATCHER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            sys.stderr.write(f"WARN: binary not found: {binary}\n")
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    return linked


def verify() -> list[str]:
    """Verify all dispatchers respond to --help. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
    failures = []

    for crate in DISPATCHER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary), "--help"],
            capture_output=True,
        )
        if result.returncode != 0:
            failures.append(crate)
            continue
        verified.append(crate)

    if failures:
        sys.stderr.write(f"FAIL: {len(failures)} dispatchers broken:\n")
        for failed_crate in failures:
            sys.stderr.write(f"  {failed_crate}\n")
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_dispatchers()
    linked = ensure_symlinks()
    verified = verify()

    sys.stdout.write(
        f"DEPLOY COMPLETE: {len(verified)} dispatchers built, "
        f"{linked} symlinked to {TOOLS_BIN}\n"
    )
