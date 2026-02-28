# Nornir Quickstart

## What Nornir Does

Nornir is a Rust workspace that validates TOML agent definitions against JSON Schema at every stage of the Bragi composition pipeline. It produces:

- **8 CLI check binaries** -- validate TOML files from the command line
- **32 PyO3 gate modules** -- Python-importable Rust validation for pipeline boundaries
- **2 writer binaries** -- schema-validated output for QC reports and glossary files
- **1 dispatcher binary** -- splits JSONL files into batches for parallel agent dispatch

The schemas are embedded at compile time. Errors are educational (what/where/found/expected/fix).

---

## Building

```bash
cd /Users/johnny/.ai/spaces/bragi/tools/nornir
cargo build --release
```

This produces all binaries in `target/release/`.

For full deployment:

```bash
# Gates + CLI checkers (after schema changes)
./tools/nornir/deploy_gates.py

# Writer tools (after adding/modifying writers)
./tools/nornir/deploy_writers.py
```

`deploy_gates.py` builds CLI binaries with cargo, builds gate modules with maturin, extracts `.so` files from wheels, creates symlinks in `~/.ai/tools/bin/`, and verifies everything works. `deploy_writers.py` builds writer binaries, symlinks, and verifies.

---

## Architecture

Rust workspace with 53 member crates organized into layers:

```
core/           5 foundation libraries (error types, format conversion, schema engine,
                path extraction, write engine)
capability/     5 feature crates (embedded schemas, IO contracts, path verification,
                gate IO operations)
gates/          32 PyO3 gate modules (cdylib crate-type, importable from Python)
cli/            8 standalone check binaries
writers/        2 enforcement output binaries
dispatchers/    1 batch splitting binary
```

**Dependency flow**: core -> capability -> gates/cli/writers/dispatchers

---

## CLI Check Tools

Each binary validates a TOML file against its schema and optionally verifies filesystem paths.

| Binary | Schema | Verifies Paths |
|---|---|---|
| `check_raw_definition` | raw-definition | No |
| `check_paths_resolved` | paths-resolved | No |
| `check_paths_verified` | paths-resolved | Yes |
| `check_includes_merged` | includes-merged | Yes |
| `check_permissions_resolved` | permissions-resolved | Yes |
| `check_universal_format` | universal-format | Yes |
| `check_universal_render` | universal-render | Yes |
| `check_anthropic_render` | anthropic-render | Yes |

Usage:
```bash
# File argument
check_raw_definition my-agent.toml

# Stdin
cat my-agent.toml | check_paths_verified

# JSON output
check_universal_format my-agent.toml --json
```

Exit codes: 0 = valid, 1 = invalid (validation errors), 2 = operational error.

---

## PyO3 Gates

Gates are Python-importable Rust modules that validate data at pipeline boundaries. Each exposes three functions:

```python
import gate_raw_definition_input

# Full validation with educational errors
result = gate_raw_definition_input.validate("/path/to/agent.toml")
# Returns: {"ok": True, "data": "<json string>", "error": None}
# Or:      {"ok": False, "data": None, "error": {"type": "...", "message": "..."}}

# Quick boolean check
valid = gate_raw_definition_input.is_valid("/path/to/agent.toml")

# Schema name
name = gate_raw_definition_input.schema_name()
# Returns: "raw-definition"
```

Three gate categories:

**Input gates** (19 gates, `gate_*_input`): Accept a file path. Read TOML from disk, validate against schema, return JSON.
```python
result = gate_raw_definition_input.validate("/path/to/agent.toml")
# result["data"] contains the validated JSON string
```

**Output gates** (12 gates, `gate_*_output`): Accept JSON data string and output file path. Validate JSON, convert to TOML, write to disk.
```python
result = gate_paths_resolved_output.validate(json_string, "/path/to/output.toml")
# Writes validated TOML to the output path
```

**Passthrough gate** (1 gate, `gate_paths_verified`): Accept input path and output path. Read TOML, validate, verify all referenced paths exist on disk, write TOML.
```python
result = gate_paths_verified.validate("/path/to/input.toml", "/path/to/output.toml")
```

---

## Writers

```bash
# Append a QC report record (schema-validated, fsync'd)
echo '{"uid":"abc","assessment":"..."}' | append_qc_report_record

# Write a glossary file (refuses overwrite, atomic write)
echo '{"term":"...","definition":"..."}' | write_glossary_file my-glossary
```

Both validate input against their embedded schema before writing. Output is `OK` on success or `FAIL:<reason>` on failure.

---

## Dispatcher

```bash
split_jsonl_batches \
  --input /path/to/data.jsonl \
  --directory batch-run-123 \
  --min-batch 35 \
  --max-batch 50
```

Creates `/tmp/batch-run-123/batch_001.jsonl`, etc. Outputs JSONL manifest to stdout.

---

## Pipeline Stage Progression

The schemas represent a composition pipeline where agent definitions progress through stages:

```
raw-definition           (authored TOML)
  -> paths-resolved      (relative paths resolved to absolute)
    -> guardrails-reduced, success-reduced, criteria-merged,
       instructions-reduced, examples-reduced, execution-merged
                         (sections reduced/merged into canonical form)
      -> includes-merged   (file includes resolved to inline content)
        -> permissions-resolved  (permission declarations resolved)
          -> universal-format    (provider-agnostic format)
            -> universal-render  (provider-agnostic render-ready)
              -> anthropic-render  (Anthropic-specific render)
```

Additionally, 7 include fragment schemas validate individual include files before merging:
include-success-criteria, include-failure-criteria, include-execution-instructions, include-example-entries, include-example-group, include-guardrails-constraints, include-guardrails-anti-patterns.

---

## Where Things Live

| Resource | Path |
|---|---|
| Nornir workspace | `/Users/johnny/.ai/spaces/bragi/tools/nornir/` |
| Schema source files | `/Users/johnny/.ai/spaces/bragi/schemas/agent-*.schema.json` |
| Include fragment schemas | `/Users/johnny/.ai/spaces/bragi/schemas/include-*.schema.json` |
| Writer schemas | `/Users/johnny/.ai/spaces/bragi/schemas/{qc-report,glossary}.schema.json` |
| Agent definitions | `/Users/johnny/.ai/spaces/bragi/definitions/agents/` |
| Deploy gates | `tools/nornir/deploy_gates.py` |
| Deploy writers | `tools/nornir/deploy_writers.py` |
| Built binaries | `tools/nornir/target/release/` |
| Deployed CLI symlinks | `~/.ai/tools/bin/check_*` |
| Deployed gate modules | `~/.ai/tools/lib/gate_*.so` |

---

## Crate Structure for Development

### Core crates (pure libraries, `rlib`)

| Crate | Purpose | Depends on |
|---|---|---|
| `error_core` | Error types, ValidationIssue, educational formatting | serde, serde_json, jsonschema, thiserror |
| `format_core` | TOML<->JSON conversion | error_core, toml |
| `schema_core` | EmbeddedValidator with OnceLock | error_core, jsonschema |
| `path_core` | Extract `path_exists_absolute` fields from schema+data | error_core, serde_json |
| `write_core` | Write engine (config, path safety, atomic writes) | error_core, schema_core |

### Capability crates (feature libraries, `rlib`)

| Crate | Purpose | Depends on |
|---|---|---|
| `schemas_embedded` | All schemas via `include_str!()` as `EmbeddedValidator` statics | schema_core |
| `path_verify` | Filesystem path existence checks (the only impure capability) | path_core, error_core |
| `io_filter` | stdin->validate->stdout/stderr filter contract | error_core |
| `io_check` | File-arg diagnostic output contract for CLI tools | error_core, serde, serde_json |
| `gate_io` | Shared gate IO: read_and_validate, validate_and_write, read_validate_write | format_core, path_verify, schema_core, error_core |

### Gate crates (`cdylib` + `rlib`, PyO3)

All gates follow the same pattern: thin wrappers around `gate_io` functions using a specific `schemas_embedded` validator. Each gate depends on `pyo3`, `schemas_embedded`, and `gate_io`.

### CLI crates (binary)

All CLI tools follow the same pattern: define a `check()` function using `format_core::toml_to_json()` + schema validation + optional path verification, then call `io_check::run_check(check)`. Each depends on `schemas_embedded`, `format_core`, `schema_core`, `path_verify`, `io_check`, `error_core`.

### Writer crates (binary)

Each writer defines a `WriterConfig` and calls `write_core::run()`. Depends on `write_core` + `schemas_embedded`.

---

## What NOT to Do

1. **Do not modify schema files without rebuilding nornir.** Schemas are embedded at compile time via `include_str!()`. If you change a `.schema.json` file, the binaries still contain the old version until you run `cargo build --release`.

2. **Do not edit `schemas_embedded/src/lib.rs` to add schemas without also creating the schema file.** The `include_str!()` paths are resolved at compile time and will cause a build failure if the target file does not exist.

3. **Do not skip path verification steps.** The `check_paths_resolved` binary validates schema structure only. Use `check_paths_verified` to also verify that referenced files exist on disk.

4. **Do not use `build_validators.py`.** That is a legacy script for the old `rust/formats/dag_step_validate_*` architecture. Use `deploy_gates.py` and `deploy_writers.py` inside `tools/nornir/` for the current nornir system.

5. **Do not hand-write validation logic in Python.** The gates exist precisely to avoid ad-hoc validation. Import the gate module and call `validate()`.

6. **Do not bypass the gate API.** Every gate returns `{"ok": bool, "data": ..., "error": ...}`. Check `ok` before using `data`. The error dict contains educational messages.

7. **Do not assume gate input/output formats.** Input gates accept a file path and return JSON. Output gates accept JSON data + output path and write TOML. The passthrough gate accepts input path + output path. Check the gate name suffix (`_input`, `_output`, or no suffix for passthrough).

---

## Adding a New Pipeline Stage

To add a new gate for a new schema:

1. Create the schema at `/Users/johnny/.ai/spaces/bragi/schemas/agent-<name>.schema.json`
2. Add an `EmbeddedValidator` entry in `capability/schemas_embedded/src/lib.rs`
3. Create the gate crate(s) under `gates/gate_<name>_{input,output}/` with `Cargo.toml` and `src/lib.rs`
4. Add the gate crate(s) to workspace members in the root `Cargo.toml`
5. Add the crate name(s) to `GATE_CRATES` in `tools/nornir/deploy_gates.py`
6. Optionally create a CLI check binary under `cli/check_<name>/`
7. Run `cargo build --release` and `deploy_gates.py`

---

## Key Design Principles

- **Schemas are the single source of truth** -- never duplicate validation rules in code
- **Educational errors** -- every validation failure tells the consumer what/where/found/expected/fix
- **Compile-time embedding** -- schemas baked into binaries, no runtime file dependencies
- **Strict purity boundaries** -- core crates have no IO; only path_verify and gate_io touch the filesystem
- **Path verification via schema annotations** -- `format: path_exists_absolute` in the schema declares which fields are paths
- **Security** -- path traversal protection on all LLM-provided filename components
- **Atomic writes** -- temp file + fsync + rename for durability
