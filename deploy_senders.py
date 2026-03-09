#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
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

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"

SENDER_CRATES = [
    "send_heartbeat",
    "send_notification",
    "send_warning",
    "send_alert",
    "send_datagram",
]


def build_senders() -> None:
    """Build sender binaries."""
    print("Building sender binaries...")
    packages = []
    for crate in SENDER_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (senders)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for sender binaries."""
    release_dir = NORNIR_DIR / "target" / "release"

    for crate in SENDER_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  Sender symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all senders can be invoked (exit 0 or 2 for usage)."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for crate in SENDER_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary)],
            capture_output=True,
        )
        # Accept exit 0 or 2 (usage) as valid
        if result.returncode not in (0, 2):
            failures.append((crate, result.returncode))
            continue
        print(f"  sender: {crate}")

    if failures:
        print(f"FAIL: {len(failures)} senders broken:", file=sys.stderr)
        for name, code in failures:
            print(f"  {name} (exit {code})", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR SENDERS")
    print("=" * 60)

    build_senders()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  Sender binaries: {NORNIR_DIR / 'target' / 'release'}")
    print(f"  Sender symlinks: {TOOLS_BIN}")
    print(f"  Senders:         {len(SENDER_CRATES)}")
