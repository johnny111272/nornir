#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""Generate a Nornir writer crate from parameters.

Usage:
    ./tools/nornir/generate_writer.py \\
        --name append_embedding_normalize_batch_20 \\
        --schema-const EMBEDDING_TARGET \\
        --format jsonl \\
        --frequency batch \\
        --output-kind fixed_file \\
        --file-path /abs/path/to/output.jsonl \\
        --batch-size 20

    ./tools/nornir/generate_writer.py \\
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
    ext: str | None,
) -> str:
    if output_kind == "fixed_file":
        if not file_path:
            print("--file-path required for fixed_file", file=sys.stderr)
            sys.exit(1)
        return (
            f'OutputPath::FixedFile(\n'
            f'            "{file_path}",\n'
            f'        )'
        )
    if output_kind == "directory_prefix":
        if not dir_path or not suffix:
            print(
                "--dir-path and --suffix required for directory_prefix",
                file=sys.stderr,
            )
            sys.exit(1)
        return (
            f'OutputPath::DirectoryPrefix {{\n'
            f'            dir: "{dir_path}",\n'
            f'            suffix: "{suffix}",\n'
            f'        }}'
        )
    if output_kind == "directory_name":
        if not dir_path or not ext:
            print(
                "--dir-path and --ext required for directory_name",
                file=sys.stderr,
            )
            sys.exit(1)
        return (
            f'OutputPath::DirectoryName {{\n'
            f'            dir: "{dir_path}",\n'
            f'            ext: "{ext}",\n'
            f'        }}'
        )
    print(f"Unknown output_kind: {output_kind}", file=sys.stderr)
    sys.exit(1)


def build_main_rs(
    name: str,
    schema_const: str,
    schema_path: str | None,
    fmt: str,
    freq: str,
    output_path_rust: str,
    batch_size: int | None,
) -> str:
    format_variant = "Jsonl" if fmt == "jsonl" else "Json"
    freq_variant = "Record" if freq == "record" else "Batch"
    batch_line = f"Some({batch_size})" if batch_size else "None"
    source_path = schema_path or "unknown"
    return (
        f"use schemas_embedded::{schema_const};\n"
        f"use write_core::{{OutputFormat, OutputPath, WriteFrequency, WriterConfig}};\n"
        f"\n"
        f"fn main() {{\n"
        f"    write_core::run(&WriterConfig {{\n"
        f'        name: "{name}",\n'
        f"        schema: &{schema_const},\n"
        f'        schema_source_path: "{source_path}",\n'
        f"        format: OutputFormat::{format_variant},\n"
        f"        frequency: WriteFrequency::{freq_variant},\n"
        f"        output: {output_path_rust},\n"
        f"        batch_size: {batch_line},\n"
        f"    }});\n"
        f"}}\n"
    )


def build_registry_entry(
    name: str,
    fmt: str,
    freq: str,
    output_kind: str,
    schema_path: str | None,
    file_path: str | None,
    dir_path: str | None,
    ext: str | None,
    suffix: str | None,
) -> str:
    lines = [
        "[[tools]]",
        f'binary_name = "{name}"',
        f'output_format = "{fmt}"',
        f'write_frequency = "{freq}"',
        f'output_path_kind = "{output_kind}"',
    ]
    if schema_path:
        lines.append(f'schema_path = "{schema_path}"')
    if output_kind == "fixed_file" and file_path:
        lines.append(f'file_path = "{file_path}"')
    elif output_kind == "directory_name" and dir_path:
        lines.append(f'directory_path = "{dir_path}"')
        if ext:
            lines.append(f'name_extension = "{ext}"')
    elif output_kind == "directory_prefix" and dir_path:
        lines.append(f'directory_path = "{dir_path}"')
        if suffix:
            lines.append(f'name_suffix = "{suffix}"')
    return "\n".join(lines)


def main() -> None:
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

    crate_dir = NORNIR_DIR / "writers" / args.name
    cargo_toml = build_cargo_toml(args.name)
    output_path_rust = build_output_path_rust(
        args.output_kind, args.file_path, args.dir_path, args.suffix, args.ext,
    )
    main_rs = build_main_rs(
        args.name, args.schema_const, args.schema_path,
        args.format, args.frequency, output_path_rust, args.batch_size,
    )

    if args.dry_run:
        print(f"--- writers/{args.name}/Cargo.toml ---")
        print(cargo_toml)
        print(f"--- writers/{args.name}/src/main.rs ---")
        print(main_rs)
    else:
        crate_dir.mkdir(parents=True, exist_ok=True)
        (crate_dir / "src").mkdir(exist_ok=True)
        (crate_dir / "Cargo.toml").write_text(cargo_toml)
        (crate_dir / "src" / "main.rs").write_text(main_rs)
        print(f"Generated: writers/{args.name}/Cargo.toml")
        print(f"Generated: writers/{args.name}/src/main.rs")

    print()
    print("=== Manual steps required ===")
    print()
    print(f'Add to workspace Cargo.toml members:')
    print(f'    "writers/{args.name}",')
    print()
    print(f'Add to tools/nornir/deploy_writers.py WRITER_CRATES:')
    print(f'    "{args.name}",')
    print()
    if args.schema_path:
        print("Add to tool_registry.toml:")
        print(build_registry_entry(
            args.name, args.format, args.frequency, args.output_kind,
            args.schema_path, args.file_path, args.dir_path, args.ext, args.suffix,
        ))
        print()


if __name__ == "__main__":
    main()
