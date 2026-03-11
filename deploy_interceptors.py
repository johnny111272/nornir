#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir interceptor binaries (traffic rewriters).

Single command: ./deploy_interceptors.py

Builds and deploys:
  - Interceptor binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all interceptor commands work immediately.
"""

import subprocess
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

INTERCEPTOR_CRATES = [
    "traffic_interceptor_rewriter",
]


def build_interceptors() -> bool:
    """Build interceptor binaries."""
    logger.info("Building interceptor binaries...")
    packages = []
    for crate in INTERCEPTOR_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (interceptors) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for interceptor binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in INTERCEPTOR_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Interceptor symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all interceptors can be invoked. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
    failures = []

    for crate in INTERCEPTOR_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            capture_output=True,
        )
        if result.returncode not in (0, 1, 2):
            failures.append(crate)
            continue
        logger.info("  interceptor: {}", crate)
        verified.append(crate)

    if failures:
        logger.error("{} interceptors broken:", len(failures))
        for failed_crate in failures:
            logger.error("  {}", failed_crate)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_interceptors()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} interceptors built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
