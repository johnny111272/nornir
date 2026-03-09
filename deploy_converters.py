#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir converter binaries (format conversion tools).

Single command: ./deploy_converters.py

Builds and deploys:
  - Converter binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all converter commands work immediately.
Verify with: echo '{"key":"value"}' | convert_json_to_toml
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

CONVERTER_CRATES = [
    "convert_json_to_toml",
]


def build_converters() -> None:
    """Build converter binaries."""
    print("Building converter binaries...")
    packages = []
    for crate in CONVERTER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (converters)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for converter binaries."""
    release_dir = NORNIR_DIR / "target" / "release"

    for crate in CONVERTER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Converter symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all converters respond to basic input."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for crate in CONVERTER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            input=b'{"test": true}',
            capture_output=True,
        )
        if result.returncode != 0:
            failures.append((crate, result.returncode))
            continue
        print(f"  converter: {crate}")

    if failures:
        print(f"FAIL: {len(failures)} converters broken:", file=sys.stderr)
        for name, code in failures:
            print(f"  {name} (exit {code})", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR CONVERTERS")
    print("=" * 60)

    build_converters()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Converter binaries: {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Converter symlinks: {TOOLS_BIN}")
    print(f"  Converters:         {len(CONVERTER_CRATES)}")
