#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir daemon binaries (long-running services).

Single command: ./deploy_daemons.py

Builds and deploys:
  - Daemon binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all daemon commands work immediately.
Start with: record_datagrams
"""

import subprocess
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

DAEMON_CRATES = [
    "record_datagrams",
]


def build_daemons() -> bool:
    """Build daemon binaries."""
    logger.info("Building daemon binaries...")
    packages = []
    for crate in DAEMON_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (daemons) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for daemon binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in DAEMON_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Daemon symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all daemons can be invoked. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
    failures = []

    for crate in DAEMON_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary), "--help"],
            capture_output=True,
        )
        if result.returncode not in (0, 1, 2):
            failures.append(crate)
            continue
        logger.info("  daemon: {}", crate)
        verified.append(crate)

    if failures:
        logger.error("{} daemons broken:", len(failures))
        for failed_crate in failures:
            logger.error("  {}", failed_crate)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_daemons()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} daemons built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
