#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir quality tools (saga, qa-report, syn).

Single command: ./tools/nornir/deploy_tools.py

Builds and deploys:
  - Quality tool binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all tool commands work immediately.
Verify with: saga --help or saga <file.py>
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

# (crate_name, binary_name) — crate is the Cargo package, binary is the output name
TOOL_CRATES = [
    ("saga_cli", "saga"),
    ("syn_cli", "syn"),
]


def build_tools() -> None:
    """Build tool binaries."""
    print("Building quality tools...")
    packages = []
    for crate, _ in TOOL_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (tools)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for tool binaries."""
    release_dir = NORNIR_DIR / "target" / "release"

    for _, binary_name in TOOL_CRATES:
        binary = release_dir / binary_name
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / binary_name
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Tool symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all tools respond to a basic invocation."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for _, binary_name in TOOL_CRATES:
        binary = release_dir / binary_name
        # saga with no args prints usage and exits 2
        result = subprocess.run(
            [str(binary)],
            capture_output=True,
        )
        # Accept exit 0 or 2 (usage) as valid
        if result.returncode not in (0, 2):
            failures.append((binary_name, result.returncode))
            continue
        print(f"  tool: {binary_name}")

    if failures:
        print(f"FAIL: {len(failures)} tools broken:", file=sys.stderr)
        for name, code in failures:
            print(f"  {name} (exit {code})", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR QUALITY TOOLS")
    print("=" * 60)

    build_tools()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Tool binaries: {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Tool symlinks: {TOOLS_BIN}")
    print(f"  Tools:         {len(TOOL_CRATES)}")
