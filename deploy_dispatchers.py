#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir dispatcher tools (batch processing binaries).

Single command: ./deploy_dispatchers.py

Builds and deploys:
  - Dispatcher binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all dispatcher commands work immediately.
Verify with: {binary_name} --help
"""

import subprocess
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

DISPATCHER_CRATES = [
    "split_jsonl_batches",
]


def build_dispatchers() -> bool:
    """Build dispatcher tool binaries."""
    logger.info("Building dispatcher binaries...")
    packages = []
    for crate in DISPATCHER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (dispatchers) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for dispatcher tools."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in DISPATCHER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Dispatcher symlinks updated in {}", TOOLS_BIN)
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
        logger.info("  dispatcher: {}", crate)
        verified.append(crate)

    if failures:
        logger.error("{} dispatchers broken:", len(failures))
        for failed_crate in failures:
            logger.error("  {}", failed_crate)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_dispatchers()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} dispatchers built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
