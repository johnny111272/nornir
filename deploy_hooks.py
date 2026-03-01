#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir hook intercept binaries.

Single command: ./tools/nornir/deploy_hooks.py

Builds and deploys:
  - Hook binaries via cargo (symlinks in ~/.ai/tools/bin/)

After running, all hook commands work immediately.
Verify with: echo '{}' | {binary_name}
"""

import subprocess
import sys
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

HOOK_CRATES = [
    "hook_intercept_subagent_tool",
    "hook_intercept_subagent_bash",
    "hook_intercept_llm_tool",
    "hook_intercept_llm_bash",
]


def build_hooks() -> None:
    """Build hook binaries."""
    print("Building hook binaries...")
    packages = []
    for crate in HOOK_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (hooks)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for hook binaries."""
    release_dir = NORNIR_DIR / "target" / "release"

    for crate in HOOK_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Hook symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all hooks respond to empty JSON input (should allow or parse-fail gracefully)."""
    release_dir = NORNIR_DIR / "target" / "release"
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
        print(f"  hook: {crate}")

    if failures:
        print(f"FAIL: {len(failures)} hooks broken:", file=sys.stderr)
        for name, code in failures:
            print(f"  {name} (exit {code})", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR HOOKS")
    print("=" * 60)

    build_hooks()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Hook binaries: {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Hook symlinks: {TOOLS_BIN}")
    print(f"  Hooks:         {len(HOOK_CRATES)}")
