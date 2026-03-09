#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
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

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

CONVERTER_CRATES = [
    "convert_json_to_toml",
]


def build_converters() -> bool:
    """Build converter binaries."""
    logger.info("Building converter binaries...")
    packages = []
    for crate in CONVERTER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (converters) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for converter binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in CONVERTER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Converter symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all converters respond to basic input. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
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
        logger.info("  converter: {}", crate)
        verified.append(crate)

    if failures:
        logger.error("{} converters broken:", len(failures))
        for name, code in failures:
            logger.error("  {} (exit {})", name, code)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_converters()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} converters built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
