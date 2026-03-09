#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Deploy Nornir validation gates and CLI check tools.

Single command: ./deploy_gates.py

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

from loguru import logger

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


def build_cli() -> bool:
    """Build CLI check tool binaries."""
    logger.info("Building CLI check tools...")
    packages = []
    for crate in CLI_CRATES:
        packages.extend(["-p", crate])
    result = subprocess.run(
        ["cargo", "build", "--release", *packages],
        cwd=NORNIR_DIR,
    )
    if result.returncode != 0:
        logger.error("cargo build (CLI) failed")
        sys.exit(1)
    logger.info("cargo build complete")
    return True


def build_gates() -> int:
    """Build PyO3 gate modules with maturin and extract .so files. Returns gates built."""
    TOOLS_LIB.mkdir(parents=True, exist_ok=True)
    wheels_dir = NORNIR_DIR / "target" / "wheels"
    built = 0

    for crate in GATE_CRATES:
        crate_dir = NORNIR_DIR / "gates" / crate
        logger.info("  maturin: {}...", crate)

        result = subprocess.run(
            ["uvx", "maturin", "build", "--release", "-i", "python3.13"],
            cwd=crate_dir,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            logger.error("maturin build {} failed", crate)
            logger.error("{}", result.stderr)
            sys.exit(1)

        wheels = sorted(wheels_dir.glob(f"{crate}-*.whl"))
        if not wheels:
            logger.error("no wheel found for {}", crate)
            sys.exit(1)

        wheel = wheels[-1]
        with zipfile.ZipFile(wheel) as zf:
            so_files = [name for name in zf.namelist() if name.endswith(".so")]
            if not so_files:
                logger.error("no .so in wheel {}", wheel.name)
                sys.exit(1)
            for so_file in so_files:
                so_name = Path(so_file).name
                dest = TOOLS_LIB / so_name
                with zf.open(so_file) as src, open(dest, "wb") as dst:
                    dst.write(src.read())
                dest.chmod(0o755)
        built += 1

    logger.info("PyO3 gates deployed to {}", TOOLS_LIB)
    return built


def ensure_symlinks() -> int:
    """Create/update symlinks in ~/.ai/tools/bin/ for CLI tools."""
    release_dir = NORNIR_DIR / "target" / "release"
    linked = 0

    for crate in CLI_CRATES:
        binary = release_dir / crate
        if not binary.exists():
            logger.warning("binary not found: {}", binary)
            continue

        link = TOOLS_BIN / crate
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(binary)
        linked += 1

    logger.info("CLI symlinks updated in {}", TOOLS_BIN)
    return linked


def verify() -> tuple[list[str], list[str]]:
    """Verify all gates and CLI tools work. Returns (verified_cli, verified_gates)."""
    release_dir = NORNIR_DIR / "target" / "release"
    failures = []
    verified_cli = []
    verified_gates = []

    for crate in CLI_CRATES:
        binary = release_dir / crate
        result = subprocess.run(
            [str(binary), "--help"],
            capture_output=True,
        )
        if result.returncode not in (0, 1):
            failures.append(f"CLI: {crate}")
            continue
        logger.info("  CLI: {}", crate)
        verified_cli.append(crate)

    for crate in GATE_CRATES:
        result = subprocess.run(
            [
                "python3.13", "-c",
                f"import sys; sys.path.insert(0, '{TOOLS_LIB}'); "
                f"import {crate}; sys.stdout.write({crate}.schema_name() + '\\n')",
            ],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            failures.append(f"gate: {crate}")
            continue
        schema = result.stdout.strip()
        logger.info("  gate: {} -> {}", crate, schema)
        verified_gates.append(crate)

    if failures:
        logger.error("{} items broken:", len(failures))
        for failed_item in failures:
            logger.error("  {}", failed_item)
        sys.exit(1)

    return verified_cli, verified_gates


if __name__ == "__main__":
    build_cli()
    gates_built = build_gates()
    linked = ensure_symlinks()
    verified_cli, verified_gates = verify()

    logger.info(
        "DEPLOY COMPLETE: {} CLI tools, {} gates built, {} symlinked to {}",
        len(verified_cli), len(verified_gates), linked, TOOLS_BIN,
    )
