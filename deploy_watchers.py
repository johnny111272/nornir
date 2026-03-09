#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir watcher binaries.

Single command: ./deploy_watchers.py

Builds and deploys:
  - Watcher binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all watcher commands work immediately.
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

WATCHER_CRATES = [
    "watch_and_diff_exchange_intercepts",
]


def build_watchers() -> None:
    """Build watcher binaries."""
    print("Building watcher tools...")
    packages = []
    for crate in WATCHER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (watchers)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for watcher tools."""
    release_dir = NORNIR_DIR / "target" / "release"

    for crate in WATCHER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Watcher symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all watchers respond to invocation."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for crate in WATCHER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            capture_output=True,
        )
        # Watchers exit 2 with usage message when called with no args
        if result.returncode == 2:
            print(f"  watcher: {crate}")
        else:
            failures.append(crate)

    if failures:
        print(f"FAIL: {len(failures)} watchers broken:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR WATCHERS")
    print("=" * 60)

    build_watchers()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Watcher binaries: {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Watcher symlinks: {TOOLS_BIN}")
    print(f"  Watchers:         {len(WATCHER_CRATES)}")
