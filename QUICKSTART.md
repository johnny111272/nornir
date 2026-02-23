# Nornir Quickstart

Nornir is the Rust-based validation and enforcement engine for Bragi's agent definition pipeline. It externalizes trust to compiled, immutable validators so that LLM compliance is proven by gates, not self-reported.

---

## What It Is

Nornir enforces JSON Schema validation at every boundary of the agent definition pipeline. The pipeline is a DAG that transforms raw agent definitions through 12 stages (raw_definition through universal_render), and nornir gates ensure data conformity at each step.

**The core principle is zero trust.** No gate trusts its input. Even if the previous gate just wrote the data, the next gate re-validates from scratch. Data on the filesystem between gates is untrusted. Each gate independently proves validity.

Four artifact types:

- **Gates** (17 PyO3 `.so` modules): Imported from Python, validate data at pipeline boundaries, handle format conversion (TOML/JSON). Deployed to `~/.ai/tools/lib/`.
- **CLI check tools** (7 binaries): Validate TOML definition files from the command line. Deployed as symlinks in `~/.ai/tools/bin/`.
- **Writers** (2 binaries): Schema-validate JSON from stdin before writing to disk. Deployed as symlinks in `~/.ai/tools/bin/`.
- **Dispatchers** (1 binary): Utility for splitting JSONL files into batches. Deployed as symlink in `~/.ai/tools/bin/`.

---

## The Pipeline DAG

The pipeline is a DAG with parallel branches, not a linear sequence:

```
                ┌─ [2] instructions ─┐
   [1] ─ [1b] ─┤                    ├─ [6] execution ─┐
                ├─ [3] examples ─────┘                 │
                │                                      ├─ [8] includes ─┐
                ├─ [4] guardrails ───┐                 │                │
                │                    ├─ [7] criteria ──┘                ├─ [10] universal ─── [11] render
                ├─ [5] success/fail ─┘                                 │
                │                                                      │
                └─ [9] permissions ────────────────────────────────────┘
```

Steps 2+3 run in parallel (join at 6). Steps 4+5 run in parallel (join at 7). Steps 6+7 join at 8. Step 9 is independent after 1b. Steps 8+9 converge at 10. Step 11 follows 10.

At TOML checkpoints, an exit gate writes TOML and the next entry gate re-reads and re-validates it (zero trust).

---

## How to Build and Deploy

### Full rebuild of validators (gates + CLI tools)

```bash
/Users/johnny/.ai/spaces/bragi/tools/deploy_nornir_validators.py
```

This runs `cargo build --release` for CLI tools and `uvx maturin build --release` for each PyO3 gate module, then symlinks binaries to `~/.ai/tools/bin/` and extracts `.so` files to `~/.ai/tools/lib/`.

### Full rebuild of writers

```bash
/Users/johnny/.ai/spaces/bragi/tools/deploy_nornir_writers.py
```

### Manual cargo build (development)

```bash
cargo build --release -p check_raw_definition -p gate_raw_definition_input
```

Run from `/Users/johnny/.ai/spaces/bragi/tools/nornir/`.

---

## How to Use: CLI Check Tools

All CLI tools accept a file path or stdin, and support `--json` for structured output.

### Validate a raw TOML definition

```bash
~/.ai/tools/bin/check_raw_definition /path/to/definition.toml
```

### Validate with path verification

```bash
~/.ai/tools/bin/check_paths_verified /path/to/definition.toml
```

### Pipe from stdin

```bash
cat definition.toml | ~/.ai/tools/bin/check_raw_definition
```

### Get structured JSON output

```bash
~/.ai/tools/bin/check_raw_definition definition.toml --json
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Valid |
| 1 | Invalid (schema validation errors) |
| 2 | Operational error (file not found, parse error) |

### Available check tools

| Tool | Schema | Path Verification |
|------|--------|-------------------|
| `check_raw_definition` | raw-definition | No |
| `check_paths_resolved` | paths-resolved | No |
| `check_paths_verified` | paths-resolved | Yes (same schema + filesystem check) |
| `check_includes_resolved` | includes-resolved | Yes |
| `check_permissions_resolved` | permissions-resolved | Yes |
| `check_universal_format` | universal-format | Yes |
| `check_universal_render` | universal-render | Yes |

---

## How to Use: PyO3 Gates (from Python)

Gates are `.so` modules installed to `~/.ai/tools/lib/`. Add that directory to `sys.path` before importing.

### Import and validate

```python
import sys
sys.path.insert(0, "/Users/johnny/.ai/tools/lib")

import gate_raw_definition_input

# Full validation
result = gate_raw_definition_input.validate(toml_string)
if result["ok"]:
    json_output = result["data"]
else:
    error_msg = result["error"]["message"]

# Quick check
is_valid = gate_raw_definition_input.is_valid(toml_string)

# Schema name
name = gate_raw_definition_input.schema_name()  # "raw-definition"
```

### Return format

```python
# Success:
{"ok": True, "data": "<validated output string>", "error": None}

# Failure:
{"ok": False, "data": None, "error": {"type": "validation_error", "message": "<educational error>"}}
```

### Gate format behavior

| Gate suffix | Input format | Output format | Example |
|-------------|-------------|---------------|---------|
| `_input` | TOML | JSON | `gate_paths_verified_input` |
| `_output` | JSON | TOML | `gate_paths_resolved_output` |
| (no suffix, passthrough) | JSON | JSON | `gate_guardrails_reduced` |
| `gate_paths_verified` (special) | TOML | TOML | Only this gate |

### Gate verification rules

- `gate_raw_definition_input` and `gate_paths_resolved_output` do NOT verify filesystem paths (paths not yet resolved).
- All other gates (from `gate_paths_verified` onward) verify `path_exists_absolute` fields against the filesystem.

---

## How to Use: Writers

Writers read JSON from stdin, validate against an embedded schema, and write to a hardcoded output path. They implement the "constrained tool pattern" -- the LLM provides JSON content, the tool handles everything else.

### Append a QC report record

```bash
echo '{"uid":"abc123","assessment":"pass","details":"..."}' | ~/.ai/tools/bin/append_qc_report_record
```

Output path: `/Users/johnny/.ai/spaces/bragi/truth/qc_semantic_report.jsonl`
The file must already exist.

### Write a glossary entry

```bash
echo '{"term":"nornir","definition":"Validation engine"}' | ~/.ai/tools/bin/write_glossary_file entry-name
```

Output path: `/Users/johnny/.ai/spaces/bragi/truth/quarantine/entry-name.json`
The file must NOT already exist (refuses to overwrite).

### For data with quotes or apostrophes, use heredoc

```bash
cat <<'RECORD' | ~/.ai/tools/bin/append_qc_report_record
{"uid":"abc123","assessment":"pass","details":"value with 'quotes'"}
RECORD
```

### Writer output protocol

```
OK           # Success (record mode)
OK:<count>   # Success (batch mode)
FAIL:<reason>  # Failure with educational guidance
```

---

## How to Use: Dispatchers

### Split JSONL into batches

```bash
~/.ai/tools/bin/split_jsonl_batches \
    --input /path/to/records.jsonl \
    --directory batch_run_id \
    --min-batch 35 \
    --max-batch 50
```

Creates `/tmp/batch_run_id/batch_001.jsonl`, `batch_002.jsonl`, etc.
Outputs JSONL manifest to stdout:

```json
{"batch":1,"file":"/tmp/batch_run_id/batch_001.jsonl","records":48}
{"batch":2,"file":"/tmp/batch_run_id/batch_002.jsonl","records":48}
```

---

## What NOT to Do

### Do not load schemas at runtime

Schemas are baked into binaries at compile time via `include_str!()`. If you modify a schema in `/Users/johnny/.ai/spaces/bragi/schemas/`, you must recompile nornir. The binaries will NOT pick up schema changes without rebuilding.

### Do not call gates as CLI commands

Gates are PyO3 Python modules (`.so` files), not executables. Import them from Python. For CLI validation, use the `check_*` tools.

### Do not try/except around gate validate()

The `validate()` function returns a dict with `ok: True/False`. It does not raise Python exceptions for validation failures. Check `result["ok"]` instead.

### Do not write custom validation logic in Python

The workspace has a strict rule: all validation uses JSON Schema files in `schemas/` validated through `jsonschema.validate()` or nornir's embedded validators. Writing inline field-checking code in Python is a session-terminating violation per workspace standards (STANDARD_OPERATING_PROCEDURES.md).

### Do not bypass writers for validated output

If a writer exists for a data type (QC reports, glossary entries), use the writer binary. It enforces schema validation, path traversal protection, and atomic writes. Do not write directly to the output files.

### Do not pass path components with traversal characters

Writer filename arguments are validated against path traversal: no `..`, no `/` or `\`, no null bytes, no leading `.`, no empty strings. These will be rejected with a `FAIL:path traversal blocked` error.

### Do not forget to deploy after changes

Any change to nornir source code or schemas requires redeployment:
- Schema changes: run both `deploy_nornir_validators.py` and `deploy_nornir_writers.py`
- Gate/CLI changes: run `deploy_nornir_validators.py`
- Writer changes: run `deploy_nornir_writers.py`

### Do not treat the pipeline as linear

The pipeline is a DAG with parallel branches. Steps 2+3 run in parallel, steps 4+5 run in parallel, step 9 is independent. Do not assume sequential ordering of all steps.

### Do not trust gate input even if the previous gate validated it

This is the zero-trust principle. Data on the filesystem between gates is untrusted. Each gate re-validates independently. This is by design, not a redundancy to optimize away.

### Do not flatten the hierarchy or simplify the composition model

The 8-level YAML composition hierarchy (field/group/array/supergroup/section/profile/composed/definition) is intentional. Each level adds capability. See REFRESH.md critical decisions.

---

## Where to Get Context

### Schemas (the source of truth for data shape)

```
/Users/johnny/.ai/spaces/bragi/schemas/agent-raw-definition.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-paths-resolved.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-includes-resolved.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-permissions-resolved.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-guardrails-reduced.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-sf-reduced.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-criteria-merged.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-instructions-reduced.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-examples-reduced.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-execution-merged.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-universal-format.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/agent-universal-render.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/qc-report.schema.json
/Users/johnny/.ai/spaces/bragi/schemas/glossary.schema.json
```

### Design documentation (read before modifying nornir)

```
/Users/johnny/.ai/spaces/bragi/documentation/pipeline/NORNIR_TOOL_DESIGN.md       # Complete design spec: gate matrix, topology, architecture
/Users/johnny/.ai/spaces/bragi/documentation/pipeline/SECURITY_RESOLUTION.md       # Permission resolution algorithm (Phase A-F)
/Users/johnny/.ai/spaces/bragi/documentation/pipeline/TRANSFORMER_LOGIC.md         # Full resolver pipeline DAG, universal format spec
/Users/johnny/.ai/spaces/bragi/documentation/toolcompose/TOOL_COMPOSE_REASONING.md # Three-layer constraints, constrained tool pattern
/Users/johnny/.ai/spaces/bragi/documentation/behavioral/REFRESH.md                 # Document map, critical decisions not to violate
/Users/johnny/.ai/spaces/bragi/documentation/pipeline/CRITICAL_DECISIONS.md        # Design decisions D1-D10, build state
```

### Deploy scripts

```
/Users/johnny/.ai/spaces/bragi/tools/deploy_nornir_validators.py
/Users/johnny/.ai/spaces/bragi/tools/deploy_nornir_writers.py
```

### Context file (full system description)

```
/Users/johnny/.ai/spaces/bragi/context/nornir.md
```

### Related systems

```
/Users/johnny/.ai/spaces/bragi/tools/draupnir/               # Schema generator (upstream of nornir)
/Users/johnny/.ai/spaces/bragi/tools/transform_to_universal/  # 11-step DAG pipeline (consumes nornir gates)
/Users/johnny/.ai/spaces/bragi/schemas/                       # JSON Schema files (embedded by nornir)
/Users/johnny/.ai/spaces/bragi/truth/                         # Output location for writers
```
