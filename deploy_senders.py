#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir sender binaries (datagram emitters).

Single command: ./deploy_senders.py

Builds and deploys:
  - Sender binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all sender commands work immediately.
Verify with: send_heartbeat <source>
"""

import subprocess
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

SENDER_CRATES = [
    "send_heartbeat",
    "send_notification",
    "send_warning",
    "send_alert",
    "send_datagram",
]


def build_senders() -> bool:
    """Build sender binaries."""
    logger.info("Building sender binaries...")
    packages = []
    for crate in SENDER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (senders) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for sender binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in SENDER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Sender symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all senders can be invoked. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
    failures = []

    for crate in SENDER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            capture_output=True,
        )
        # Accept exit 0, 1 (missing args), or 2 (usage) as valid
        if result.returncode not in (0, 1, 2):
            failures.append(crate)
            continue
        logger.info("  sender: {}", crate)
        verified.append(crate)

    if failures:
        logger.error("{} senders broken:", len(failures))
        for failed_crate in failures:
            logger.error("  {}", failed_crate)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_senders()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} senders built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
