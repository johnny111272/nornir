#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir quality tools (saga, syn).

Single command: ./deploy_tools.py

Builds and deploys:
  - Quality tool binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all tool commands work immediately.
Verify with: saga --help or saga <file.py>
"""

import subprocess
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

# (crate_name, binary_name) — crate is the Cargo package, binary is the output name
TOOL_CRATES = [
    ("saga_cli", "saga"),
    ("syn_cli", "syn"),
]


def build_tools() -> bool:
    """Build tool binaries."""
    logger.info("Building quality tools...")
    packages = []
    for crate, _ in TOOL_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (tools) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for tool binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for _, binary_name in TOOL_CRATES:
        binary = release_dir / binary_name
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / binary_name
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Tool symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all tools respond to basic invocation. Returns list of verified binary names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
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
        logger.info("  tool: {}", binary_name)
        verified.append(binary_name)

    if failures:
        logger.error("{} tools broken:", len(failures))
        for name, code in failures:
            logger.error("  {} (exit {})", name, code)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_tools()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} tools built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
