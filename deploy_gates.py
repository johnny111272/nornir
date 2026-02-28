#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Deploy Nornir validation gates and CLI check tools.

Single command: ./tools/nornir/deploy_gates.py

Builds and deploys:
  - 8 CLI check tools via cargo (symlinks in ~/.ai/tools/bin/)
  - 32 PyO3 gate modules via maturin (deployed to ~/.ai/tools/lib/)

After running, `check_*` CLI commands and `import gate_*` Python imports
work immediately.
"""

import subprocess
import sys
import zipfile
from pathlib import Path

NORNIR_DIR = Path(__file__).resolve().parent
TOOLS_BIN = Path.home() / ".ai" / "tools" / "bin"
TOOLS_LIB = Path.home() / ".ai" / "tools" / "lib"

CLI_CRATES = [
    "check_raw_definition",
    "check_paths_resolved",
    "check_paths_verified",
    "check_includes_merged",
    "check_permissions_resolved",
    "check_universal_format",
    "check_universal_render",
    "check_anthropic_render",
]

GATE_CRATES = [
    # Input gates (read TOML from disk, validate, return JSON)
    "gate_raw_definition_input",
    "gate_paths_verified_input",
    "gate_includes_merged_input",
    "gate_permissions_resolved_input",
    "gate_universal_format_input",
    "gate_universal_render_input",
    "gate_anthropic_render_input",
    "gate_guardrails_reduced_input",
    "gate_success_reduced_input",
    "gate_criteria_merged_input",
    "gate_instructions_reduced_input",
    "gate_examples_reduced_input",
    "gate_execution_merged_input",
    # Output gates (validate JSON, write TOML to disk)
    "gate_paths_resolved_output",
    "gate_includes_merged_output",
    "gate_permissions_resolved_output",
    "gate_universal_format_output",
    "gate_universal_render_output",
    "gate_anthropic_render_output",
    "gate_guardrails_reduced_output",
    "gate_success_reduced_output",
    "gate_criteria_merged_output",
    "gate_instructions_reduced_output",
    "gate_examples_reduced_output",
    "gate_execution_merged_output",
    # Passthrough gate (read TOML, validate, verify paths, write TOML)
    "gate_paths_verified",
    # Include fragment input gates (validate include TOML files)
    "gate_include_success_criteria_input",
    "gate_include_failure_criteria_input",
    "gate_include_execution_instructions_input",
    "gate_include_example_entries_input",
    "gate_include_example_group_input",
    "gate_include_guardrails_constraints_input",
    "gate_include_guardrails_anti_patterns_input",
]


def build_cli() -> None:
    """Build CLI check tool binaries."""
    print("Building CLI check tools...")
    packages = []
    for crate in CLI_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        print("FAIL: cargo build (CLI)", file=sys.stderr)
        sys.exit(1)
    print("  cargo build complete")


def build_gates() -> None:
    """Build PyO3 gate modules with maturin and extract .so files."""
    TOOLS_LIB.mkdir(parents=True, exist_ok=True)
    wheels_dir = NORNIR_DIR / "target" / "wheels"

    for crate in GATE_CRATES:
        crate_dir = NORNIR_DIR / "gates" / crate
        print(f"  maturin: {crate}...")

        result = subprocess.run(
            ["uvx", "maturin", "build", "--release", "-i", "python3.13"],
            cwd=crate_dir,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            print(f"FAIL: maturin build {crate}", file=sys.stderr)
            print(result.stderr, file=sys.stderr)
            sys.exit(1)

        wheels = sorted(wheels_dir.glob(f"{crate}-*.whl"))
        if not wheels:
            print(f"FAIL: no wheel found for {crate}", file=sys.stderr)
            sys.exit(1)

        wheel = wheels[-1]
        with zipfile.ZipFile(wheel) as zf:
            so_files = [n for n in zf.namelist() if n.endswith(".so")]
            if not so_files:
                print(f"FAIL: no .so in wheel {wheel.name}", file=sys.stderr)
                sys.exit(1)
            for so_file in so_files:
                so_name = Path(so_file).name
                dest = TOOLS_LIB / so_name
                with zf.open(so_file) as src, open(dest, "wb") as dst:
                    dst.write(src.read())
                dest.chmod(0o755)

    print(f"  PyO3 gates deployed to {TOOLS_LIB}")


def ensure_symlinks() -> None:
    """Create/update symlinks in ~/.ai/tools/bin/ for CLI tools."""
    release_dir = NORNIR_DIR / "target" / "release"

    for crate in CLI_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            print(f"WARN: binary not found: {binary}", file=sys.stderr)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)

    print(f"  CLI symlinks updated in {TOOLS_BIN}")


def verify() -> None:
    """Verify all gates and CLI tools work."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []

    for crate in CLI_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary), "--help"],
            capture_output=True,
        )
        if result.returncode not in (0, 1):
            failures.append(f"CLI: {crate}")
            continue
        print(f"  CLI: {crate}")

    for crate in GATE_CRATES:
        result = subprocess.run(
            [
                "python3.13", "-c",
                f"import sys; sys.path.insert(0, '{TOOLS_LIB}'); "
                f"import {crate}; print({crate}.schema_name())",
            ],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            failures.append(f"gate: {crate}")
            continue
        schema = result.stdout.strip()
        print(f"  gate: {crate} -> {schema}")

    if failures:
        print(f"FAIL: {len(failures)} items broken:", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    print("=" * 60)
    print("DEPLOY NORNIR GATES + CLI CHECKERS")
    print("=" * 60)

    build_cli()
    build_gates()
    ensure_symlinks()

    print()
    print("Verifying...")
    verify()

    print()
    print("=" * 60)
    print("DEPLOY COMPLETE")
    print("=" * 60)
    print(f"  CLI binaries:    {NORNIR_DIR / 'target' / 'release'}")
    print(f"  CLI symlinks:    {TOOLS_BIN}")
    print(f"  PyO3 gates:      {TOOLS_LIB}")
    print(f"  CLI tools:       {len(CLI_CRATES)}")
    print(f"  Gates:           {len(GATE_CRATES)}")
