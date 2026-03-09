# Nornir Naming Conventions

Every name in this workspace is an instruction to the next LLM session. A wrong name generates wrong code that reinforces the wrong name. Naming violations compound exponentially across sessions.

## Binary Names

All binaries follow **verb-prefix** naming. The verb tells you what category the binary belongs to and what it does. No exceptions.

### Verb Prefixes

| Prefix | Category | Directory | What it does |
|--------|----------|-----------|--------------|
| `append_` | writers | `writers/` | Accepts data on stdin, appends to a file with fsync |
| `write_` | writers | `writers/` | Accepts data on stdin, writes a new file atomically |
| `check_` | validators | `cli/` | Validates a file against a schema, exits 0/1/2 |
| `convert_` | converters | `converters/` | Reads one format on stdin, writes another on stdout |
| `gate_` | gates | `gates/` | PyO3 module: validates at a pipeline stage boundary |
| `hook_` | hooks | `hooks/` | Intercepts an LLM tool call for security/quality |
| `send_` | senders | `senders/` | Fire-and-forget datagram to hlidskjalf socket |
| `generate_` | generators | (varies) | Produces output files from templates or computation |
| `rewrite_` | rewriters | `rewriters/` | Reads JSON on stdin, modifies content, writes JSON on stdout |
| `split_` | dispatchers | `dispatchers/` | Splits input into batches for parallel processing |
| `watch_` | watchers | `watchers/` | Monitors files for changes and emits datagrams |

**Specialist tools** (`saga`, `syn`) are proper nouns — they do not take a verb prefix. These are rare and require explicit justification.

### Binary Name Structure

```
{verb}_{domain}_{specifics}
```

**Examples:**
- `append_truth_qc_report_record` — verb=append, domain=truth_qc_report, specifics=record
- `check_universal_render` — verb=check, domain=universal_render
- `convert_json_to_toml` — verb=convert, domain=json, specifics=to_toml
- `hook_pre_llm_bash` — verb=hook, domain=pre_llm, specifics=bash
- `send_alert` — verb=send, domain=alert
- `gate_raw_definition_input` — verb=gate, domain=raw_definition, specifics=input

### Rules

1. **Underscores only.** Never hyphens. Shell completion, grep, and import paths all break on hyphens.
2. **Binary name = package name = directory name.** All three MUST match exactly. If the directory is `writers/append_raw_jsonl/`, the Cargo.toml `name` is `append_raw_jsonl`, and the `[[bin]] name` is `append_raw_jsonl`.
3. **Exception: specialist tools.** `saga_cli` package produces `saga` binary. `syn_cli` package produces `syn` binary. The directory name matches the package name (`cli/saga/` contains `saga_cli`). These are the ONLY exceptions and exist because `saga` and `syn` are proper nouns.

## Crate Names

### Core Libraries (`core/`)

```
{domain}_core
```

Core crates are pure Rust libraries with no I/O side effects. They contain the foundational types and logic.

| Name | Purpose |
|------|---------|
| `error_core` | ValidationIssue types, educational error formatting |
| `format_core` | JSON/YAML/TOML/TOON conversion with diagnostics |
| `schema_core` | EmbeddedValidator with lazy-static schema loading |
| `path_core` | Path field extraction from schema+data |
| `write_core` | Config-driven atomic writes with fsync |
| `saga_core` | SanityReport + Issue types, .qa sidecar generation, directory walker |
| `gleipnir_core` | Tree-sitter AST guardrail engine |
| `diff_core` | Line-level diff and TOML block extraction |
| `report_render` | QA report grouping, formatting, serialization for consumers |

**The `_core` suffix is mandatory.** It signals "this is a pure library, safe to depend on from anywhere."

### Capability Libraries (`capability/`)

Capability crates provide specific features and may have I/O side effects. No mandatory suffix — names describe the capability.

| Name | Purpose |
|------|---------|
| `schemas_embedded` | All schema definitions via `include_str!()` |
| `path_verify` | Filesystem path existence checks |
| `io_filter` | stdin-validate-stdout filter contract |
| `io_check` | File-arg diagnostic output contract |
| `gate_io` | Gate I/O orchestration (read/validate/write) |
| `hook_io` | Hook input parsing, response formatting, shared rule types |
| `socket_emit` | Fire-and-forget Unix socket datagram emission |
| `intercept_io` | PyO3 module: json_to_toml + append_jsonl_line for bifrost |

### Gate Modules (`gates/`)

```
gate_{stage}_{direction}
```

- `{stage}` matches the schema name with underscores: `raw_definition`, `paths_resolved`, `universal_render`
- `{direction}` is `input` (TOML→JSON) or `output` (JSON→TOML)
- Special case: `gate_paths_verified` (no direction suffix) is a passthrough gate: read→validate→verify→write. This exists alongside `gate_paths_verified_input` which is a standard input gate for the same schema. Both are valid — one passes TOML through, the other returns JSON.
- Exception: `gate_include_*_input` gates validate include fragments

## Directory Names

### Top-Level Categories

Every crate lives under exactly one category directory. The category determines what kind of artifact the crate produces.

| Directory | Contains | Artifact Type |
|-----------|----------|---------------|
| `core/` | `*_core` library crates | `rlib` (static library) |
| `capability/` | Feature library crates | `rlib` (static library) |
| `gates/` | `gate_*` PyO3 modules | `cdylib` + `rlib` (Python extension) |
| `cli/` | `check_*` + specialist binaries | Executable |
| `writers/` | `append_*` / `write_*` binaries | Executable |
| `hooks/` | `hook_*` binaries | Executable |
| `senders/` | `send_*` binaries | Executable |
| `converters/` | `convert_*` binaries | Executable |
| `dispatchers/` | `split_*` binaries | Executable |
| `watchers/` | Watcher binaries | Executable |
| `schemas/` | `.schema.json` files | Data (not compiled) |

### Crate Directory Name = Crate Name

The directory under the category MUST match the crate's package name in Cargo.toml.

```
writers/append_raw_jsonl/Cargo.toml  →  name = "append_raw_jsonl"
core/diff_core/Cargo.toml            →  name = "diff_core"
senders/send_alert/Cargo.toml        →  name = "send_alert"
```

**No exceptions.** If you find a mismatch, it is a bug.

## Schema File Names

```
schemas/agents/{stage-name}.schema.json
schemas/tools/{tool-name}.schema.json
```

- Kebab-case for schema filenames: `agent-raw-definition.schema.json`
- This is the ONLY place kebab-case appears in nornir
- The kebab maps to underscores in code: `raw_definition`

## Rust Module Names

- Snake_case everywhere: `pub mod convert`, `pub fn json_to_toml`
- Match the domain language: `format_core::convert::json_to_toml`
- Function names describe the transformation: `validate_and_write`, `read_and_validate`

## What Never Appears in Nornir Names

- **Hyphens in crate/binary names.** Use underscores.
- **Generic words:** `utils`, `helpers`, `common`, `misc`, `tools`, `lib`
- **Version suffixes:** `_v2`, `_new`, `_old`, `_legacy`
- **Abbreviations** unless universally understood: `io` is OK, `fmt` is not
- **Numbers** unless they encode a real parameter: `batch_20` means batch size 20, not version 20
