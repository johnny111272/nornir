#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir writer tools (enforcement output binaries).

Single command: ./tools/nornir/deploy_writers.py

Builds and deploys:
  - Writer binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all writer commands work immediately.
Verify with: {binary_name} --help
Inspect schema with: {binary_name} --dump-schema
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

WRITER_CRATES = [
    "append_truth_qc_report_record",
    "write_truth_glossary_record",
    "append_embedding_normalize_batch_20",
    "append_interview_summaries_record",
    "append_raw_jsonl",
]


def build_writers() -> None:
    """Build writer tool binaries."""
    print("Building writer tools...")
    packages = []
    for crate in WRITER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (writers)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for writer tools."""
    release_dir = NORNIR_DIR / "target" / "release"

    for crate in WRITER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Writer symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all writers respond to --help."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for crate in WRITER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary), "--help"],
            capture_output=True,
        )
        if result.returncode != 0:
            failures.append(crate)
            continue
        print(f"  writer: {crate}")

    if failures:
        print(f"FAIL: {len(failures)} writers broken:", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR WRITERS")
    print("=" * 60)

    build_writers()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Writer binaries: {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Writer symlinks: {TOOLS_BIN}")
    print(f"  Writers:         {len(WRITER_CRATES)}")
