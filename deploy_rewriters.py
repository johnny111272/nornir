#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir rewriter binaries.

Single command: ./deploy_rewriters.py

Builds and deploys:
  - rewrite_compaction_summary (compaction request rewriter)
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

REWRITER_CRATES = [
    "rewrite_compaction_summary",
]


def build() -> None:
    """Build rewriter binaries."""
    print("Building rewriter binaries...")
    packages = []
    for crate in REWRITER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (rewriters)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for rewriter binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    TOOLS_BIN.mkdir(parents=True, exist_ok=True)

    for crate in REWRITER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all rewriter binaries work."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for crate in REWRITER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            ["sh", "-c", f'echo \'{{"system":[],"tools":[],"messages":[]}}\' | {binary}'],
            capture_output=True,
        )
        if result.returncode != 0:
            failures.append(crate)
            continue
        print(f"  rewriter: {crate}")

    if failures:
        print(f"FAIL: {len(failures)} rewriters broken:", file=sys.stderr)
        for failed_crate in failures:
            print(f"  {failed_crate}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR REWRITERS")
    print("=" * 60)

    build()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Binaries:    {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Symlinks:    {TOOLS_BIN}")
    print(f"  Rewriters:   {len(REWRITER_CRATES)}")
