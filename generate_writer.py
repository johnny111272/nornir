#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Generate a Nornir writer crate from parameters.

Usage:
    ./generate_writer.py \\
        --name append_embedding_normalize_batch_20 \\
        --schema-const EMBEDDING_TARGET \\
        --format jsonl \\
        --frequency record \\
        --output-kind fixed_file \\
        --file-path /abs/path/to/output.jsonl \\
        --batch-size 20

    ./generate_writer.py \\
        --name append_interview_summaries_record \\
        --schema-const SUMMARIES \\
        --format jsonl \\
        --frequency record \\
        --output-kind directory_prefix \\
        --dir-path /abs/path/to/dir \\
        --suffix .summaries.jsonl

Generates:
    writers/{name}/Cargo.toml
    writers/{name}/src/main.rs

Prints (does NOT auto-modify):
    - schemas_embedded lines to add
    - workspace Cargo.toml member to add
    - tool_registry.toml entry to add
    - deploy script WRITER_CRATES entry to add
"""

import argparse
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent


def build_cargo_toml(name: str) -> str:
    return f"""[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{name}"
path = "src/main.rs"

[dependencies]
write_core = {{ path = "../../core/write_core" }}
schemas_embedded = {{ path = "../../capability/schemas_embedded" }}
"""


def build_output_path_rust(
    output_kind: str,
    file_path: str | None,
    dir_path: str | None,
    suffix: str | None,
    extension: str | None,
) -> str:
    if output_kind == "fixed_file":
        if not file_path:
            logger.error("--file-path required for fixed_file")
            sys.exit(1)
        return (
            f'OutputPath::FixedFile(\n'
            f'            "{file_path}",\n'
            f'        )'
        )
    if output_kind == "directory_prefix":
        if not dir_path or not suffix:
            logger.error("--dir-path and --suffix required for directory_prefix")
            sys.exit(1)
        return (
            f'OutputPath::DirectoryPrefix {{\n'
            f'            dir: "{dir_path}",\n'
            f'            suffix: "{suffix}",\n'
            f'        }}'
        )
    if output_kind == "directory_name":
        if not dir_path or not extension:
            logger.error("--dir-path and --ext required for directory_name")
            sys.exit(1)
        return (
            f'OutputPath::DirectoryName {{\n'
            f'            dir: "{dir_path}",\n'
            f'            ext: "{extension}",\n'
            f'        }}'
        )
    logger.error("Unknown output_kind: {}", output_kind)
    sys.exit(1)


def build_main_rs(
    config: argparse.Namespace,
    output_path_rust: str,
) -> str:
    format_variant = "Jsonl" if config.format == "jsonl" else "Json"
    freq_variant = "Record" if config.frequency == "record" else "Batch"
    batch_line = f"Some({config.batch_size})" if config.batch_size else "None"
    source_path = config.schema_path or "unknown"
    return (
        f"use schemas_embedded::{config.schema_const};\n"
        f"use write_core::{{OutputFormat, OutputPath, WriteFrequency, WriterConfig}};\n"
        f"\n"
        f"fn main() {{\n"
        f"    write_core::run(&WriterConfig {{\n"
        f'        name: "{config.name}",\n'
        f"        schema: &{config.schema_const},\n"
        f'        schema_source_path: "{source_path}",\n'
        f"        format: OutputFormat::{format_variant},\n"
        f"        frequency: WriteFrequency::{freq_variant},\n"
        f"        output: {output_path_rust},\n"
        f"        batch_size: {batch_line},\n"
        f"    }});\n"
        f"}}\n"
    )


def build_registry_entry(config: argparse.Namespace) -> str:
    lines = [
        "[[tools]]",
        f'binary_name = "{config.name}"',
        f'output_format = "{config.format}"',
        f'write_frequency = "{config.frequency}"',
        f'output_path_kind = "{config.output_kind}"',
    ]
    if config.schema_path:
        lines.append(f'schema_path = "{config.schema_path}"')
    if config.output_kind == "fixed_file" and config.file_path:
        lines.append(f'file_path = "{config.file_path}"')
    elif config.output_kind == "directory_name" and config.dir_path:
        lines.append(f'directory_path = "{config.dir_path}"')
        if config.ext:
            lines.append(f'name_extension = "{config.ext}"')
    elif config.output_kind == "directory_prefix" and config.dir_path:
        lines.append(f'directory_path = "{config.dir_path}"')
        if config.suffix:
            lines.append(f'name_suffix = "{config.suffix}"')
    return "\n".join(lines)


def generate(config: argparse.Namespace) -> bool:
    """Generate writer crate files. Returns True on success."""
    crate_dir = NORNIR_DIR / "writers" / config.name
    cargo_toml = build_cargo_toml(config.name)
    output_path_rust = build_output_path_rust(
        config.output_kind, config.file_path, config.dir_path,
        config.suffix, config.ext,
    )
    main_rs = build_main_rs(config, output_path_rust)

    if config.dry_run:
        sys.stdout.write(f"--- writers/{config.name}/Cargo.toml ---\n")
        sys.stdout.write(cargo_toml)
        sys.stdout.write(f"--- writers/{config.name}/src/main.rs ---\n")
        sys.stdout.write(main_rs)
    else:
        crate_dir.mkdir(parents=True, exist_ok=True)
        (crate_dir / "src").mkdir(exist_ok=True)
        (crate_dir / "Cargo.toml").write_text(cargo_toml)
        (crate_dir / "src" / "main.rs").write_text(main_rs)
        logger.info("Generated: writers/{}/Cargo.toml", config.name)
        logger.info("Generated: writers/{}/src/main.rs", config.name)

    sys.stdout.write("\n=== Manual steps required ===\n\n")
    sys.stdout.write(f'Add to workspace Cargo.toml members:\n')
    sys.stdout.write(f'    "writers/{config.name}",\n\n')
    sys.stdout.write(f'Add to deploy_writers.py WRITER_CRATES:\n')
    sys.stdout.write(f'    "{config.name}",\n\n')

    if config.schema_path:
        sys.stdout.write("Add to tool_registry.toml:\n")
        sys.stdout.write(build_registry_entry(config))
        sys.stdout.write("\n")

    return True


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate a Nornir writer crate")
    parser.add_argument("--name", required=True, help="Binary name")
    parser.add_argument("--schema-const", required=True, help="Rust const in schemas_embedded")
    parser.add_argument("--format", required=True, choices=["jsonl", "json"])
    parser.add_argument("--frequency", required=True, choices=["record", "batch"])
    parser.add_argument(
        "--output-kind",
        required=True,
        choices=["fixed_file", "directory_prefix", "directory_name"],
    )
    parser.add_argument("--file-path", help="Absolute path for fixed_file")
    parser.add_argument("--dir-path", help="Directory for directory_prefix/directory_name")
    parser.add_argument("--suffix", help="Suffix for directory_prefix")
    parser.add_argument("--ext", help="Extension for directory_name")
    parser.add_argument("--batch-size", type=int, help="Batch size limit")
    parser.add_argument("--schema-path", help="Schema .json path for registry entry")
    parser.add_argument("--dry-run", action="store_true", help="Print files without writing")

    args = parser.parse_args()
    generate(args)
