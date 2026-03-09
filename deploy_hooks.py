#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir hook intercept binaries.

Single command: ./deploy_hooks.py

Builds and deploys:
  - Hook binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all hook commands work immediately.
Verify with: echo '{}' | {binary_name}
"""

import subprocess
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

HOOK_CRATES = [
    "hook_pre_subagent_tool",
    "hook_pre_subagent_bash",
    "hook_pre_llm_tool",
    "hook_pre_llm_bash",
    "hook_post_llm_tool",
]


def build_hooks() -> bool:
    """Build hook binaries."""
    logger.info("Building hook binaries...")
    packages = []
    for crate in HOOK_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (hooks) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for hook binaries."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in HOOK_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("Hook symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> list[str]:
    """Verify all hooks respond to empty JSON input. Returns list of verified crate names."""
    release_dir = NORNIR_DIR / "target" / "release"
    verified = []
    failures = []

    for crate in HOOK_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            input=b'{"tool_name":"Test","tool_input":{}}',
            capture_output=True,
        )
        if result.returncode != 0:
            failures.append((crate, result.returncode))
            continue
        logger.info("  hook: {}", crate)
        verified.append(crate)

    if failures:
        logger.error("{} hooks broken:", len(failures))
        for name, code in failures:
            logger.error("  {} (exit {})", name, code)
        sys.exit(1)

    return verified


if __name__ == "__main__":
    build_hooks()
    linked = ensure_symlinks()
    verified = verify()

    logger.info(
        "DEPLOY COMPLETE: {} hooks built, {} symlinked to {}",
        len(verified), linked, TOOLS_BIN,
    )
