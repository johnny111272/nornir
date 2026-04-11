#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Parse GLEIPNIR_MATRIX.md and generate gleipnir_matrix.toml.

Run from the gleipnir_core directory:
    ./generate_matrix_toml.py
"""

import sys
from pathlib import Path

EMOJI_TO_SEVERITY = {
    "\U0001f534": "blocked",   # 🔴
    "\U0001f7e0": "error",     # 🟠
    "\U0001f7e1": "warning",   # 🟡
}

V1_COLUMNS = {
    "Script": "script",
    "Test": "test",
    "DataStr": "data_structure",
    "UnsImp": "unsafe_impure",
    "UnsPur": "unsafe_pure",
    "ImpFn": "impure_function",
    "PurFn": "pure_function",
    "Outside": "outside",
}

V2_COLUMNS = {
    "Str": "structure",
    "Gen": "structure_gen",
    "Model": "structure_model",
    "Config": "structure_config",
    "Exm": "structure_example",
    "Pure": "pure",
    "PurDis": "pure_dispatch",
    "Imp": "impure",
    "ImpDis": "impure_dispatch",
    "Trans": "transform",
    "TrnDis": "transform_dispatch",
    "Orch": "orchestrate",
    "OrchDis": "orchestrate_dispatch",
    "Entry": "entry_point",
}


def parse_cell(cell: str) -> str | None:
    """Return severity string or None if not applied."""
    stripped = cell.strip()
    for emoji, severity in EMOJI_TO_SEVERITY.items():
        if emoji in stripped:
            return severity
    return None


def parse_table(
    lines: list[str],
    column_map: dict[str, str],
    version: str,
) -> dict[str, dict[str, list[str]]]:
    """Parse a markdown table into {classification: {severity: [check_names]}}."""
    result: dict[str, dict[str, list[str]]] = {}
    columns: list[str] = []

    for line in lines:
        stripped_line = line.strip()
        if not stripped_line.startswith("|"):
            continue

        cells = [cell.strip() for cell in stripped_line.split("|")]
        cells = cells[1:-1]  # strip empty first/last from leading/trailing |

        if not cells:
            continue

        # Header row: contains column names
        if cells[0].strip() == "Check":
            columns = []
            for cell in cells[1:]:
                header_name = cell.strip()
                if header_name in column_map:
                    columns.append(f"{version}.{column_map[header_name]}")
                else:
                    columns.append("")
            # Initialize result entries
            for classification_key in columns:
                if classification_key:
                    result[classification_key] = {"blocked": [], "error": [], "warning": []}
            continue

        # Separator row
        if cells[0].startswith("---") or cells[0].startswith("-"):
            continue

        # Category header row (bold text)
        if "**" in cells[0]:
            continue

        # Data row
        check_name = cells[0].strip()
        if not check_name or not columns:
            continue

        for index, cell in enumerate(cells[1:]):
            if index >= len(columns) or not columns[index]:
                continue
            severity = parse_cell(cell)
            if severity is not None:
                result[columns[index]][severity].append(check_name)

    return result


def generate_toml(all_entries: dict[str, dict[str, list[str]]]) -> str:
    """Generate TOML string from parsed matrix."""
    lines = [
        "# Gleipnir Check Matrix — generated from GLEIPNIR_MATRIX.md",
        "#",
        "# DO NOT EDIT — edit GLEIPNIR_MATRIX.md and run generate_matrix_toml.py",
        "",
    ]

    # Sort keys: v1 first, then v2
    v1_keys = sorted(name for name in all_entries if name.startswith("v1."))
    v2_keys = sorted(name for name in all_entries if name.startswith("v2."))

    for key_group in [v1_keys, v2_keys]:
        for classification_key in key_group:
            entry = all_entries[classification_key]
            lines.append(f"[{classification_key}]")
            for severity in ["blocked", "error", "warning"]:
                checks = entry[severity]
                if checks:
                    quoted = ", ".join(f'"{check}"' for check in checks)
                    lines.append(f'{severity} = [{quoted}]')
                else:
                    lines.append(f"{severity} = []")
            lines.append("")

    return "\n".join(lines)


def main() -> str:
    """Parse the markdown matrix, write the TOML, return a summary message."""
    matrix_path = Path(__file__).parent / "GLEIPNIR_MATRIX.md"
    output_path = Path(__file__).parent / "gleipnir_matrix.toml"

    content = matrix_path.read_text()
    all_lines = content.split("\n")

    # Split into V1 and V2 sections
    v1_start = None
    v2_start = None
    for index, line in enumerate(all_lines):
        if "V1 Classifications" in line:
            v1_start = index
        if "V2 Classifications" in line:
            v2_start = index

    if v1_start is None or v2_start is None:
        raise ValueError("Could not find V1/V2 section headers")

    v1_lines = all_lines[v1_start:v2_start]
    v2_lines = all_lines[v2_start:]

    v1_entries = parse_table(v1_lines, V1_COLUMNS, "v1")
    v2_entries = parse_table(v2_lines, V2_COLUMNS, "v2")

    all_entries = {**v1_entries, **v2_entries}
    toml_content = generate_toml(all_entries)

    output_path.write_text(toml_content)

    total_checks = sum(
        len(entry["blocked"]) + len(entry["error"]) + len(entry["warning"])
        for entry in all_entries.values()
    )
    return f"Generated {output_path.name}: {len(all_entries)} classifications, {total_checks} check assignments"


if __name__ == "__main__":
    sys.stdout.write(main() + "\n")
