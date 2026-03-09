#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
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

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

WATCHER_CRATES = [
    "watch_and_diff_exchange_intercepts",
]


def build_watchers() -> bool:
    """Build watcher binaries."""
    logger.info("Building watcher tools...")
    packages = []
    for crate in WATCHER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (watchers) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for watcher tools."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in WATCHER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Watcher symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all watchers respond to invocation. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
    failures = []

    for crate in WATCHER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            capture_output=True,
        )
        # Watchers exit 2 with usage message when called with no args
        if result.returncode == 2:
            logger.info("  watcher: {}", crate)
            verified.append(crate)
        else:
            failures.append(crate)

    if failures:
        logger.error("{} watchers broken:", len(failures))
        for failed_crate in failures:
            logger.error("  {}", failed_crate)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_watchers()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} watchers built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
