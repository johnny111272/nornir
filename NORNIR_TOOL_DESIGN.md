# Nornir: Pipeline Gate Toolkit

Nornir is a Rust toolkit that provides compiled, immutable validation gates for the agent definition pipeline. Named after the Norns of Norse mythology (the weavers of fate who determine what shall be), Nornir determines whether data passes or is blocked at every stage of the pipeline.

This document is the complete design specification. It is self-contained. A fresh LLM session with no prior context should be able to understand and implement the full system from this document alone.

---

## The Problem Nornir Solves

The agent definition pipeline transforms TOML definition files through multiple stages before generating Claude Code agent prompts. At each stage, an LLM performs creative work (resolving paths, expanding file includes, calculating permissions, assembling final output). Between stages, the data must be validated to ensure the LLM's output is correct.

**The trust problem:** LLMs cannot be trusted to self-report compliance. They will claim success when output is malformed, skip requirements to "record success," and compound errors across stages. The user cannot read every intermediate file to verify correctness.

**The solution:** Externalize trust to compiled Rust validators. Every stage boundary has an immutable, compiled gate that proves the data is valid before it passes. The gates are not advisory — they block invalid data. Trust lives in the compiled binary, not in LLM self-reporting.

---

## Core Concepts

### The Pipeline

The agent definition pipeline has **6 checkpoint stages**, each represented as a TOML file on the filesystem:

```
Stage 1: raw-definition         (raw TOML input)
Stage 2: paths-resolved           (all paths absolutized)
Stage 3: paths-verified           (all paths confirmed to exist on filesystem)
Stage 4: includes-resolved        (all file includes expanded, sections merged)
Stage 5: permissions-resolved     (tool permissions intersected with allowed set)
Stage 6: universal-format          (final assembled output)
```

Between stages, LLMs perform creative work in JSON. The pipeline converts TOML to JSON at stage entry, validates, and converts back to TOML at stage exit.

### Two Tool Types

**CLI Check Tools (6 total):** Static validators for TOML files at rest on the filesystem. Used during authoring for spot-checking. You point a check tool at a TOML file and it tells you whether that file is valid for its stage. Diagnostic output for humans.

**PyO3 Filter Gates (15 unique definitions):** Active pipeline filters used for enforcement. Each gate does exactly one thing: prove that data crossing a boundary is valid. Gates come in three format variants:

| Variant | What it does |
|---------|-------------|
| TOML-to-JSON | Read TOML checkpoint, convert to JSON, validate schema, verify paths |
| JSON-to-JSON | Validate schema, verify paths, pass through (or block) |
| JSON-to-TOML | Validate schema, verify paths, convert to TOML, write checkpoint |

### Zero-Trust Design

**Every gate validates. Every gate after paths-verified also verifies filesystem paths.**

No gate trusts its input. Even if the previous gate just wrote the data, the next gate re-validates. The data sat on the filesystem between gates — it could have been corrupted by the user, by the LLM, or by a bug. Each gate independently proves validity.

This means:
- Gate N writes validated TOML to a checkpoint file
- Gate N+1 reads that same file and re-validates from scratch
- If the file was modified between gates, the re-validation catches it
- The pipeline is self-healing: corruption is caught at the next boundary

### Format Rules

Every stage boundary writes its output to the filesystem and the next stage reads it back. All intermediate files are inspectable. The format choice (TOML vs JSON) determines which stages get human-friendly output:

**TOML intermediates:** Stages most likely to need manual inspection (the 6 Red checkpoints). These use an output gate (JSON-to-TOML) to write and an input gate (TOML-to-JSON) to re-read and re-validate.

**JSON intermediates:** Stages less likely to need manual inspection (the fan-out branches). These use an output gate that writes validated JSON and an input gate (same definition, separate instance) that re-reads and re-validates. The gate definition is reused but two instances run — one writes, one reads.

In both cases: two gate instances, filesystem in between, zero trust. The difference is only the serialization format on disk.

### Path Verification

The agent definition schema annotates path fields with `format: path_exists_absolute`. This custom format annotation means: "this field contains an absolute filesystem path that must exist."

- **Before paths-verified stage:** Path fields may be relative or unresolved. No filesystem verification.
- **From paths-verified stage onward:** Every gate extracts all `path_exists_absolute` fields from the data using the schema as a map, then checks that every referenced file exists on the filesystem. If ANY path is missing, the gate blocks.

This is baked into every gate from `gate_paths_verified` onward. It is not optional. It is not configurable. If the schema says the path must exist, the gate checks.

---

## The Gate Matrix

15 unique gate definitions. Each gate is defined by: what format conversion it performs, what schema it validates against, and whether it verifies filesystem paths.

### Naming Convention

Gate names follow the pattern: `gate_{purpose}` with a `_input` or `_output` suffix for directional gates. Bidirectional gates (both conversions or neither) have no suffix. The gate name describes what the gate *does*, which usually matches the schema name but not always (e.g., `gate_paths_verified` validates against the `paths-resolved` schema but additionally verifies filesystem existence).

- **`_input`** = TOML-to-JSON only (reads a TOML checkpoint into JSON)
- **`_output`** = JSON-to-TOML only (writes JSON out to a TOML checkpoint)
- **no suffix** = bidirectional (either both conversions, or JSON-to-JSON passthrough)

| # | Name | Input Conv | Validation | Verification | Output Conv | Schema |
|---|------|-----------|------------|-------------|------------|--------|
| 1 | `gate_raw_definition_input` | TOML-to-JSON | yes | no | -- | raw-definition |
| 2 | `gate_paths_resolved_output` | -- | yes | no | JSON-to-TOML | paths-resolved |
| 3 | `gate_paths_verified` | TOML-to-JSON | yes | yes | JSON-to-TOML | paths-resolved |
| 4 | `gate_paths_verified_input` | TOML-to-JSON | yes | yes | -- | paths-resolved |
| 5 | `gate_guardrails_reduced` | -- | yes | yes | -- | guardrails-reduced |
| 6 | `gate_sf_reduced` | -- | yes | yes | -- | sf-reduced |
| 7 | `gate_criteria_merged` | -- | yes | yes | -- | criteria-merged |
| 8 | `gate_instructions_reduced` | -- | yes | yes | -- | instructions-reduced |
| 9 | `gate_examples_reduced` | -- | yes | yes | -- | examples-reduced |
| 10 | `gate_execution_merged` | -- | yes | yes | -- | execution-merged |
| 11 | `gate_includes_resolved_output` | -- | yes | yes | JSON-to-TOML | includes-resolved |
| 12 | `gate_includes_resolved_input` | TOML-to-JSON | yes | yes | -- | includes-resolved |
| 13 | `gate_permissions_resolved_output` | -- | yes | yes | JSON-to-TOML | permissions-resolved |
| 14 | `gate_permissions_resolved_input` | TOML-to-JSON | yes | yes | -- | permissions-resolved |
| 15 | `gate_universal_format_output` | -- | yes | yes | JSON-to-TOML | universal-format |

### Reading the Matrix

- **Input Conv:** If the gate reads from a TOML checkpoint, it converts TOML-to-JSON. Otherwise `--` means the input is already JSON.
- **Validation:** Every gate validates against its schema. Always yes.
- **Verification:** All gates from `gate_paths_verified` (#3) onward verify `path_exists_absolute` fields against the filesystem. `gate_raw_definition_input` (#1) and `gate_paths_resolved_output` (#2) do not (paths are not yet resolved).
- **Output Conv:** If the gate writes to a TOML checkpoint, it converts JSON-to-TOML. Otherwise `--` means the output stays as JSON.
- **Schema:** The JSON Schema file this gate validates against. Located at `schemas/{name}.schema.json`.

### Gate Type Classification

From the matrix, four gate types emerge:

| Type | Gates | Pattern |
|------|-------|---------|
| TOML-to-JSON (input) | `gate_raw_definition_input`, `gate_paths_verified_input`, `gate_includes_resolved_input`, `gate_permissions_resolved_input` | Read checkpoint, convert, validate, [verify] |
| JSON-to-TOML (output) | `gate_paths_resolved_output`, `gate_includes_resolved_output`, `gate_permissions_resolved_output`, `gate_universal_format_output` | Validate, [verify], convert, write checkpoint |
| JSON-to-JSON (bidirectional) | `gate_guardrails_reduced`, `gate_sf_reduced`, `gate_criteria_merged`, `gate_instructions_reduced`, `gate_examples_reduced`, `gate_execution_merged` | Validate, verify, pass through or block |
| TOML-to-TOML (bidirectional) | `gate_paths_verified` | Read, convert, validate, verify, convert, write |

`gate_paths_verified` is the only TOML-to-TOML gate. See "The Mechanical Stage" below for why.

### Checkpoint Pairs

TOML checkpoints create gate pairs — an exit gate writes the checkpoint, an entry gate re-reads and re-validates it:

| Checkpoint | Exit Gate | Entry Gate | Schema |
|-----------|----------|------------|--------|
| Red 2: paths-resolved | `gate_paths_resolved_output` | `gate_paths_verified` (combined) | paths-resolved |
| Red 3: paths-verified | `gate_paths_verified` (combined) | `gate_paths_verified_input` | paths-resolved |
| Red 4: includes-resolved | `gate_includes_resolved_output` | `gate_includes_resolved_input` | includes-resolved |
| Red 5: permissions-resolved | `gate_permissions_resolved_output` | `gate_permissions_resolved_input` | permissions-resolved |
| Red 6: universal-format | `gate_universal_format_output` | (end) | universal-format |

`gate_paths_verified` is special: it serves as BOTH the entry gate from Red 2 AND the exit gate to Red 3. See next section.

---

## Pipeline Topology

### Left Side: Linear Entry Stages

```
[TOML] Red 1: raw-definition
  |
  gate_raw_definition_input (TOML-to-JSON, validate raw-definition schema)
  |
  :  LLM work: resolve relative paths to absolute paths
  |
  gate_paths_resolved_output (JSON-to-TOML, validate paths-resolved schema)
  |
[TOML] Red 2: agent paths validated
  |
  gate_paths_verified (TOML-to-JSON-to-TOML, validate paths-verified, verify paths exist)
  |
[TOML] Red 3: agent paths verified
```

### The Mechanical Stage (`gate_paths_verified`)

`gate_paths_verified` is the only gate with BOTH input and output conversion. It is also the only stage with NO LLM work between entry and exit. Path verification is purely mechanical: "does this file exist on the filesystem?" No LLM judgment needed.

**Why it exists as a separate stage despite being mechanical:**

The Red 2 checkpoint (paths-resolved) must be preserved as a debuggable intermediate. If path absolutization succeeds but a required file is missing from the filesystem, you need to inspect Red 2 to see: "paths were correctly absolutized, but this specific file doesn't exist." Without the Red 2 checkpoint, you would only know "something between raw definition and paths-verified failed."

`gate_paths_verified` combines what would normally be two gates (TOML-to-JSON entry + JSON-to-TOML exit) into one, because there is no LLM work between them. In practice it runs as: TOML-to-JSON | validate | verify | JSON-to-TOML.

### Right Side: Fan-Out Through Include Resolution

After Red 3, the pipeline fans out into parallel branches. Each branch extracts a section from the definition, resolves file includes in that section, and validates the result.

```
[TOML] Red 3: agent paths verified
  |
  gate_paths_verified_input (TOML-to-JSON, validate paths-verified, verify paths)
  |  |  |  |  |
  |  |  |  |  +-- permissions extraction path (see Permissions below)
  |  |  |  |
  |  |  |  +-- LLM: resolve guardrails includes
  |  |  |      gate_guardrails_reduced (JSON, guardrails-reduced)
  |  |  |
  |  |  +-- LLM: resolve process_failure includes
  |  |      gate_sf_reduced (JSON, sf-reduced)
  |  |
  |  +-- LLM: merge guardrails + sf
  |      gate_criteria_merged (JSON, criteria-merged)
  |
  +----+----+
  |    |    |
  |    |    +-- LLM: resolve instructions includes
  |    |        gate_instructions_reduced (JSON, instructions-reduced)
  |    |
  |    +-- LLM: resolve examples includes
  |        gate_examples_reduced (JSON, examples-reduced)
  |
  +-- LLM: merge instructions + examples + execution
      gate_execution_merged (JSON, execution-merged)

  gate_criteria_merged + gate_execution_merged outputs merge:
  |
  LLM: merge criteria + execute into complete definition
  |
  gate_includes_resolved_output (JSON-to-TOML, includes-resolved, verify paths)
  |
[TOML] Red 4: includes resolved
```

**CRITICAL: `gate_paths_verified_input` is reused.** It is a single compiled gate definition that is deployed 5 times in the pipeline — once at the entry of each fan-out branch (guardrails, process_failure, instructions, examples, and the permissions extraction path). All 5 instances read the same Red 3 TOML file, independently validate it, and independently verify paths. Zero trust means each branch proves its own input.

**The fan-out branches produce JSON.** `gate_guardrails_reduced` through `gate_execution_merged` are JSON-to-JSON validators. They validate the intermediate results of include resolution. The LLM work between gates transforms valid input JSON into valid output JSON. We do not name or constrain the LLM work — only the gate boundaries.

**Convergence structure:**
- Guardrails + process_failure converge into `gate_criteria_merged`
- Instructions + examples converge into `gate_execution_merged`
- Criteria + execute converge into the final merge, validated by `gate_includes_resolved_output`

### Permissions Phase

The permissions phase has a DAG structure. It starts in parallel with the fan-out (reading from Red 3 via Gate 4) and completes after includes are resolved (reading from Red 4 via Gate 12).

```
gate_paths_verified_input (5th instance, reading Red 3)
  |
  LLM: extract permissions requested from definition
  |
  ... waits for fan-out to complete and Red 4 to exist ...
  |
gate_includes_resolved_input (TOML-to-JSON, validate includes-resolved, verify paths)
  |
  LLM: calculate intersection of requested permissions vs allowed tools
  |
gate_permissions_resolved_output (JSON-to-TOML, permissions-resolved, verify paths)
  |
[TOML] Red 5: permissions resolved
```

The LLM needs both inputs — the permissions extracted from the raw definition (via `gate_paths_verified_input`) and the resolved includes (via `gate_includes_resolved_input`) — to calculate which tools the agent is actually allowed to use.

### Final Assembly

```
[TOML] Red 5: permissions resolved
  |
gate_permissions_resolved_input (TOML-to-JSON, validate permissions-resolved, verify paths)
  |
  LLM: assemble final universal format
  |
gate_universal_format_output (JSON-to-TOML, universal-format, verify paths)
  |
[TOML] Red 6: universal format
```

---

## CLI Check Tools (Red Gates)

6 CLI tools for checking TOML files at rest. Each corresponds to a pipeline checkpoint stage.

| Name | Checks | Schema |
|------|--------|--------|
| `check_raw_definition` | Raw agent definition | raw-definition |
| `check_paths_resolved` | Paths validated (absolutized) | paths-resolved |
| `check_paths_verified` | Paths verified (exist on filesystem) | paths-verified |
| `check_includes_resolved` | Includes resolved (all sections merged) | includes-resolved |
| `check_permissions_resolved` | Permissions resolved (intersection calculated) | permissions-resolved |
| `check_universal_format` | Universal format (final output) | universal-format |

**Behavior:** Read TOML file from argument, validate against schema, verify paths (for Red 3-6), print educational diagnostic, exit 0 (valid) or 1 (invalid).

**Use case:** During authoring. An LLM or human is building a definition file and wants to check "is this valid yet?" without running the full pipeline. Point the check tool at the file and get immediate feedback.

These are the same schemas used by the blue filter gates. The check tools validate TOML at rest; the filter gates validate data in motion.

---

## Schemas

11 unique schemas, each a JSON Schema file at `schemas/agent-{name}.schema.json`:

| Schema | Stage | Existing? |
|--------|-------|-----------|
| raw-definition | Raw input | existing |
| paths-resolved | Paths absolutized + paths verified (same schema, gate adds fs check) | existing |
| guardrails-reduced | Guardrails includes expanded | existing |
| sf-reduced | Success/failure includes expanded | existing |
| criteria-merged | Guardrails + SF merged | existing |
| instructions-reduced | Instructions includes expanded | existing |
| examples-reduced | Examples includes expanded | existing |
| execution-merged | Instructions + examples merged | existing |
| includes-resolved | All sections fully merged | existing |
| permissions-resolved | Permissions intersected | NEW |
| universal-format | Final assembled output | NEW |

The 9 existing schemas were generated by Draupnir (the schema generator, already complete). 2 new schemas need to be created for the stages added by this redesign.

**Note:** There is no separate `paths-verified` schema. The `paths-resolved` schema already carries `format: path_exists_absolute` annotations on path fields. Gates 3+ use the same schema but additionally run filesystem verification against those annotated fields. The schema defines the shape; the gate decides whether to enforce existence.

**Schema annotations:** Schemas use `format: path_exists_absolute` on fields that contain absolute filesystem paths. This annotation is the signal for gates to verify that the referenced files exist. All gates from `gate_paths_verified` onward enforce this check.

---

## Nornir Architecture: Core and Capability Layers

Nornir is structured as a Rust workspace with two layers:

### Layer 0: Core Crates (Pure Libraries)

All core crates are `crate-type = ["rlib"]`. No IO, no PyO3, no side effects (except path_verify which checks filesystem).

```
core/
  error_core/      Error types, educational formatting
  format_core/     TOML<->JSON conversion (pure functions)
  schema_core/     JSON Schema validation engine (EmbeddedValidator + OnceLock)
  path_core/       Extract path_exists_absolute fields from data using schema as map
```

### Layer 1: Capability Crates (Composable Building Blocks)

```
capability/
  schemas_embedded/   All 12 pipeline schemas baked in via include_str!()
  path_verify/        path_core + filesystem existence check (the only impure operation)
  io_filter/          stdin/stdout/stderr filter contract (for pipeline gates)
  io_check/           file-arg diagnostic output contract (for CLI check tools)
```

### Layer 2: Deployment (Gate Binaries and PyO3 Module)

Each of the 15 gate definitions is a thin composition of core + capability:

```rust
// Example: gate_paths_verified_input (TOML-to-JSON, paths-verified, verify paths)
fn gate_paths_verified_input(input: &str) -> Result<String, NornirError> {
    let json = format_core::toml_to_json(input)?;
    let result = schemas_embedded::PATHS_VERIFIED.validate(&json)?;
    if !result.valid { return Err(...) }
    path_verify::verify_paths(
        schemas_embedded::PATHS_VERIFIED.schema_json(),
        &json
    )?;
    Ok(json)
}
```

Each gate differs only in:
1. **Which schema** is embedded (from schemas_embedded)
2. **Which format conversions** are needed (from format_core)
3. **Whether path verification is active** (from path_verify — yes for all gates from `gate_paths_verified` onward)

The gate logic is trivially composable from these three decisions. No gate-specific business logic exists.

### Deployment Targets

Each gate definition produces:
- A **PyO3 function** for pipeline enforcement (called from Python orchestration code)
- Optionally a **CLI binary** for standalone use

The 6 CLI check tools (Red gates) are separate binaries built from `io_check` + the appropriate schema.

---

## Design Principles

### Trust Externalization
Validation trust lives in compiled, immutable Rust binaries. Not in LLM self-reporting, not in Python scripts, not in configuration files. The gates are the proof mechanism. If data passes a gate, it is proven valid. If it doesn't pass, it is blocked with an educational error explaining exactly what's wrong.

### Zero Trust at Every Boundary
No gate trusts its input, even if the previous gate just validated and wrote it. Data on the filesystem is untrusted. Data in memory between pipeline stages is untrusted. Each gate proves validity independently.

### Schema-Driven
Every validation decision comes from the JSON Schema file. The schema is the source of truth. Gates do not contain ad-hoc validation logic. If a new field needs validation, the schema changes and every gate automatically enforces it.

### Educational Errors
When a gate blocks, it does not just say "invalid." It says: where in the data the problem is (JSON pointer), what rule was violated, what was found, what was expected, and how to fix it. This follows the "Fail Hard, Fail Educationally" principle from the existing validator infrastructure.

### Composition Over Configuration
Gates are not configurable. They are composed from fixed primitives. You do not pass flags to change gate behavior. You compose a new gate from the same building blocks. This makes each gate deterministic and auditable.

### TOML as Pipeline Lingua Franca
All checkpoints are TOML. TOML is the format humans and LLMs read and write. JSON is the internal working format for schema validation (because JSON Schema validates JSON). Gates handle the conversion transparently. From the outside, the pipeline is TOML-in, TOML-out.

---

## Existing Code to Adopt

The existing Rust code at `rust/formats/validators/` contains patterns that Nornir improves upon:

| Existing | Adopt | Improve |
|----------|-------|---------|
| `validator_core/src/lib.rs` | `EmbeddedValidator`, `OnceLock` pattern | Use `thiserror` instead of `String` errors |
| `validator_errors/src/lib.rs` | `ValidationIssue`, educational formatting | Move into `error_core`, derive `Serialize` |
| `pyresult/src/lib.rs` | PyO3 result protocol pattern | Defer to pylib phase |
| 9 validator crates | Schema embedding via `include_str!()` | Single `schemas_embedded` crate replaces 9 duplicates |

The existing validators have 73 lines of identical boilerplate duplicated across 9 crates. Nornir eliminates this duplication with a single composition layer.

---

## Build Phases

### Phase 0: Scaffold
Create workspace structure, all Cargo.toml files, empty lib.rs stubs. `cargo check --workspace` passes.

### Phase 1: error_core + format_core
Error types, educational formatting, TOML<->JSON conversion. Tests for round-trip conversion and error display.

### Phase 2: schema_core
EmbeddedValidator with OnceLock pattern. Tests with inline schema strings.

### Phase 3: path_core
Schema-walking path extractor for `format: path_exists_absolute` fields. Tests against real schemas.

### Phase 4: schemas_embedded + path_verify
Embed all schemas, implement filesystem verification. Tests for all schemas compiling and path verification working.

### Phase 5: io_filter + io_check
IO contracts for filter gates and check tools. Integration tests composing all layers.

### Phase 6: Gate Deployment (Planned Separately)
Compose the 15 gate definitions and 6 CLI check tools from core + capability layers. Build PyO3 module. This phase requires the core and capability layers to be complete first.
