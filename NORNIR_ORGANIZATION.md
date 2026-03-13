# Nornir Organization

Nornir is a Rust monorepo workspace at `~/.ai/smidja/nornir/`. It produces validated binaries, Python extension modules, and libraries that form the tooling infrastructure for the entire `.ai` ecosystem.

## What Nornir Is

A **validation-as-code** system. Schemas are the source of truth. They are embedded in binaries at compile time via `include_str!()`. Every binary validates its input against a schema before processing. Invalid data never reaches the processing logic.

Nornir currently produces:
- 8 CLI validation binaries (`check_*`)
- 33 PyO3 gate modules (`gate_*` — Python-importable Rust)
- 5 writer binaries (`append_*`, `write_*`)
- 5 hook binaries (`hook_*` — LLM security interceptors)
- 5 sender binaries (`send_*` — datagram emitters)
- 1 rewriter binary (`rewrite_*`)
- 1 converter binary (`convert_*`)
- 1 dispatcher binary (`split_*`)
- 2 specialist tools (`saga`, `syn`)
- 20 library crates (10 core + 10 capability)

## Architecture: Three-Tier Dependency Model

```
Tier 1: CORE (pure libraries, no I/O)
    error_core, format_core, schema_core, path_core,
    saga_core, gleipnir_core, diff_core, datagram_types,
    report_render_core, compaction_inject_core

         │
         ▼
Tier 2: CAPABILITY (feature libraries, may have I/O)
    schemas_embedded, path_verify_io, io_filter, io_check,
    gate_io, hook_io, datagram_io, intercept_io,
    write_engine, saga_runner
         │
         ▼
Tier 3: BINARIES (executables and Python extensions)
    gates/*, cli/*, writers/*, hooks/*, senders/*,
    converters/*, rewriters/*, watchers/*, dispatchers/*
```

**Rules:**
- Tier 1 crates depend ONLY on other Tier 1 crates and workspace external deps
- Tier 2 crates depend on Tier 1 and other Tier 2 crates
- Tier 3 crates depend on Tier 1 and Tier 2 (never on other Tier 3 crates)
- No circular dependencies. Ever.

## Directory Map

```
nornir/
├── Cargo.toml              # Workspace manifest (ALL members listed here)
├── Cargo.lock              # Dependency lock
├── NORNIR_NAMING.md        # Naming conventions (READ FIRST)
├── NORNIR_ORGANIZATION.md  # This file
├── NORNIR_BUILDING_AND_COMPOSITION.md  # How to build things correctly
├── MANDATORY_READ_BEFORE_CODING.md  # Compliance gate (references all three)
├── QUICKSTART.md           # Feature overview and usage
├── CLAUDE.md               # Session instructions
├── PLAN.md                 # Current development roadmap
│
├── schemas/                # JSON Schema definitions (source of truth)
│   ├── agents/             # Agent pipeline stage schemas
│   │   ├── agent-raw-definition.schema.json
│   │   ├── agent-paths-resolved.schema.json
│   │   └── ...             # 20+ schema files
│   └── tools/              # Tool output schemas
│       ├── datagram.schema.json
│       └── ...             # 5 schema files
│
├── core/                   # Tier 1: Pure libraries (no I/O)
│   ├── error_core/         # Base error types, educational formatting
│   ├── format_core/        # JSON/YAML/TOML/TOON conversion
│   ├── schema_core/        # JSON Schema validation engine
│   ├── path_core/          # Path field extraction from schema+data
│   ├── saga_core/          # SanityReport + Issue types, pure path functions
│   ├── gleipnir_core/      # Tree-sitter AST guardrail engine
│   ├── diff_core/          # Line-level diff + block extraction
│   ├── datagram_types/     # Datagram, DatagramKind, Priority types
│   ├── report_render_core/ # QA report grouping, formatting, serialization for consumers
│   └── compaction_inject_core/ # Compaction summary instructions injection
│
├── capability/             # Tier 2: Feature libraries (may have I/O)
│   ├── schemas_embedded/   # All schemas via include_str!()
│   ├── path_verify_io/     # Filesystem path existence checks
│   ├── io_filter/          # stdin→validate→stdout contract
│   ├── io_check/           # File-arg diagnostic output contract
│   ├── gate_io/            # Gate orchestration (read/validate/write)
│   ├── hook_io/            # Hook input parsing + response format + shared rule types
│   ├── datagram_io/        # Dual-transport datagram emission
│   ├── intercept_io/       # PyO3 module: json_to_toml + append_jsonl_line for bifrost
│   ├── write_engine/         # Atomic write engine (config, fsync)
│   └── saga_runner/        # QA report generation, directory walker, sidecar I/O
│
├── gates/                  # Tier 3: PyO3 pipeline gate modules
│   ├── gate_raw_definition_input/
│   ├── gate_raw_definition_output/  # (if exists)
│   ├── gate_paths_verified/         # Passthrough gate
│   └── ...                          # 33 gate crates total
│
├── cli/                    # Tier 3: CLI validation + specialist tools
│   ├── check_raw_definition/
│   ├── check_paths_resolved/
│   ├── check_paths_verified/
│   ├── check_includes_merged/
│   ├── check_permissions_resolved/
│   ├── check_universal_format/
│   ├── check_universal_render/
│   ├── check_anthropic_render/
│   ├── saga_cli/            # Package: saga_cli, binary: saga
│   └── syn_cli/             # Package: syn_cli, binary: syn
│
├── writers/                # Tier 3: Schema-validated output writers
│   ├── append_truth_qc_report_record/
│   ├── append_interview_summaries_record/
│   ├── append_embedding_normalize_batch_20/
│   ├── append_raw_jsonl/
│   └── write_truth_glossary_record/
│
├── rewriters/              # Tier 3: JSON request rewriters (stdin → stdout)
│   └── rewrite_compaction_summary/
│
├── hooks/                  # Tier 3: LLM security interceptors
│   ├── hook_pre_llm_tool/
│   ├── hook_pre_llm_bash/
│   ├── hook_post_llm_tool/
│   ├── hook_pre_subagent_tool/
│   └── hook_pre_subagent_bash/
│
├── senders/                # Tier 3: Datagram emitters
│   ├── send_alert/
│   ├── send_warning/
│   ├── send_notification/
│   ├── send_heartbeat/
│   └── send_datagram/
│
├── converters/             # Tier 3: Format conversion binaries
│   └── convert_json_to_toml/
│
├── watchers/               # Tier 3: File/event watcher binaries
│   └── watch_and_diff_exchange_intercepts/  # Gutted — clean redesign pending
│
├── dispatchers/            # Tier 3: Batch processing
│   └── split_jsonl_batches/
│
├── daemons/                # Tier 3: Background daemon binaries
│   └── record_datagrams/
│
├── interceptors/           # Tier 3: Traffic interceptor binaries
│   └── traffic_interceptor_rewriter/
│
├── deploy_gates.py         # Builds + deploys CLI checks + PyO3 gates
├── deploy_hooks.py         # Builds + deploys hook binaries
├── deploy_writers.py       # Builds + deploys writer binaries
├── deploy_rewriters.py     # Builds + deploys rewriter binaries
├── deploy_senders.py       # Builds + deploys sender binaries
├── deploy_converters.py    # Builds + deploys converter binaries
├── deploy_watchers.py      # Builds + deploys watcher binaries
├── deploy_dispatchers.py   # Builds + deploys dispatcher binaries
├── deploy_interceptors.py  # Builds + deploys interceptor binaries
├── deploy_daemons.py       # Builds + deploys daemon binaries
├── deploy_tools.py         # Builds + deploys specialist tools (saga, syn)
└── generate_writer.py      # Helper: scaffolds new writer crates
```

## Architecture Patterns

### Binary Structure

All Tier 3 binaries follow the same pattern: `parse_args()` and `run()` return `Result`, `main()` is the only exit point.

```rust
fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => { eprintln!("{e}"); process::exit(2); }
    };
    match run(&args) {
        Ok(msg) => println!("{msg}"),
        Err(e) => { eprintln!("{e}"); process::exit(1); }
    }
}
```

**No `process::exit()` in helper functions.** Helpers return `Result`, main matches on it. This makes all logic testable and composable.

### Declarative Writers

Writers are ~16-line binaries. Define a `WriterConfig` and call `write_engine::run()`:

```rust
fn main() {
    match write_engine::run(&WriterConfig { name: "...", schema: &SCHEMA, ... }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => { eprintln!("{msg}"); process::exit(1); }
    }
}
```

### Hook Pattern

Hook binaries use `hook_io::run_hook(decide)` where `decide` is a pure function `fn(&HookInput) -> HookDecision`. Shared rule parsing lives in `hook_io::rules`.

### Pure/Impure Separation

- saga_core defines pure types (SanityReport, Issue) and path functions (no I/O)
- saga_runner generates reports, walks directories, manages sidecars (I/O — used by saga, syn)
- report_render_core formats/groups reports (pure — used by syn, svalinn, future consumers)
- datagram_types defines Datagram, DatagramKind, Priority (pure — used by diff_core)
- datagram_io emits datagrams (impure — used by all senders, syn, hooks)

## Test Coverage

All tests must pass. Run `cargo test -p {crate_name}` per crate, or exclude gate crates for a full workspace run.

Zero-test Tier 3 crates are trivial delegation (~16–25 lines): declarative writers, simple senders, check_* binaries, convert_json_to_toml, and the 33 gate crates. Testing them would test the framework, not the crate.

## Common Terms

| Term | Meaning |
|------|---------|
| **gate** | A PyO3 Rust module that validates data at a pipeline stage boundary. Input gates read TOML→validate→return JSON. Output gates accept JSON→validate→write TOML. |
| **check** | A CLI binary that validates a file against a schema. Exits 0 (valid), 1 (invalid), 2 (operational error). |
| **writer** | A binary that accepts JSON on stdin and writes it to a specific file format with schema validation and fsync. |
| **sender** | A binary that constructs and emits a datagram to the hlidskjalf Unix socket. Fire-and-forget. |
| **hook** | A binary invoked by Claude Code's PreToolUse/PostToolUse hooks. Validates LLM actions for security. |
| **converter** | A binary that reads one format on stdin and writes another on stdout. Pure transformation. |
| **watcher** | A binary that monitors files for changes and emits datagrams when significant events occur. |
| **core crate** | A pure Rust library in `core/`. No I/O. Safe to depend on from anywhere. |
| **capability crate** | A feature library in `capability/`. May have I/O. Provides shared behavior to Tier 3 crates. |
| **workspace dep** | A dependency declared in the root `Cargo.toml` `[workspace.dependencies]`. All crates reference these with `{ workspace = true }` to ensure uniform versions. |
| **schema** | A JSON Schema file in `schemas/`. The single source of truth for data validation. Embedded in binaries at compile time. |
| **pipeline stage** | One step in the agent definition composition pipeline. Raw definition → paths resolved → sections reduced/merged → includes merged → permissions resolved → universal format → universal render → anthropic render. |
| **datagram** | A JSON message sent to the hlidskjalf Unix socket at `/tmp/ai_logger.sock`. Fields: timestamp, source, kind, priority, workspace, detail, speech, payload. |

## Workspace Cargo.toml

The root `Cargo.toml` is the single source of truth for:
1. **All workspace members** — every crate must be listed
2. **All shared dependency versions** — pinned in `[workspace.dependencies]`
3. **Resolver version** — always `2`

### Current Workspace Dependencies

```toml
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
toml = "0.8"
toon-format = "0.4"
jsonschema = "0.29"
thiserror = "2.0"
pyo3 = { version = "0.22", features = ["extension-module"] }
regex = "1.11"
jaq-interpret = { version = "1.5", features = ["serde_json"] }
jaq-parse = "1.0"
```

**Adding a new external dependency:**
1. Add it to `[workspace.dependencies]` in the root Cargo.toml
2. Reference it in crate Cargo.toml as `{dep} = { workspace = true }`
3. Never pin a version in a crate's own Cargo.toml if a workspace version exists

## Deploy Scripts

Every binary category has a deploy script. Binaries are NOT deployed by running `cargo build` alone — the deploy scripts handle symlink creation, verification, and (for gates) Python module extraction.

| Script | Deploys | Binary Target |
|--------|---------|---------------|
| `deploy_gates.py` | `check_*` CLI tools + `gate_*` PyO3 modules | `~/.ai/tools/bin/` + `~/.ai/tools/lib/` |
| `deploy_hooks.py` | `hook_*` binaries | `~/.ai/tools/bin/` |
| `deploy_writers.py` | `append_*` + `write_*` binaries | `~/.ai/tools/bin/` |
| `deploy_rewriters.py` | `rewrite_*` binaries | `~/.ai/tools/bin/` |
| `deploy_senders.py` | `send_*` binaries | `~/.ai/tools/bin/` |
| `deploy_converters.py` | `convert_*` binaries | `~/.ai/tools/bin/` |
| `deploy_watchers.py` | `watch_*` binaries | `~/.ai/tools/bin/` |
| `deploy_dispatchers.py` | `split_*` binaries | `~/.ai/tools/bin/` |
| `deploy_interceptors.py` | Interceptor binaries | `~/.ai/tools/bin/` |
| `deploy_daemons.py` | Daemon binaries | `~/.ai/tools/bin/` |
| `deploy_tools.py` | `saga`, `syn` specialist tools | `~/.ai/tools/bin/` |

**After adding a new crate:**
1. Add the crate to the appropriate deploy script's crate list
2. Run the deploy script
3. Verify the binary appears in `~/.ai/tools/bin/` and runs

**Never build and symlink manually in the terminal.** The deploy scripts exist for a reason — they handle all deployment steps consistently. A manual `cargo build && ln -s` will work today and be forgotten tomorrow.

## Adding a New Crate

### Step-by-step:

1. **Choose the correct category directory** based on what the crate produces (see Directory Map)
2. **Name the crate** following NORNIR_NAMING.md conventions
3. **Create the directory** under the correct category: `{category}/{crate_name}/`
4. **Create `Cargo.toml`** with:
   - `name` matching directory name
   - `[[bin]] name` matching directory name (if binary)
   - `edition = "2021"`
   - Dependencies using `{ workspace = true }` for shared deps
   - Internal deps using `{ path = "../../{tier}/{crate}" }`
5. **Create `src/main.rs`** (binary) or `src/lib.rs`** (library)
6. **Add to workspace `Cargo.toml`** members list
7. **Add to the appropriate deploy script** crate list
8. **Run `cargo check`** to verify compilation
9. **Run the deploy script** to build and deploy

### Cargo.toml Template (binary crate)

```toml
[package]
name = "{verb}_{domain}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{verb}_{domain}"
path = "src/main.rs"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
# Internal deps:
# format_core = { path = "../../core/format_core" }
```

### Cargo.toml Template (core library)

```toml
[package]
name = "{domain}_core"
version = "0.1.0"
edition = "2021"

[lib]
name = "{domain}_core"
crate-type = ["rlib"]

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

## Deployment Targets

| Target | Path | Contents |
|--------|------|----------|
| Binary symlinks | `~/.ai/tools/bin/` | Symlinks to `nornir/target/release/{binary}` |
| PyO3 modules | `~/.ai/tools/lib/` | `.so`/`.dylib` files extracted from maturin wheels |
| Schemas (source) | `nornir/schemas/` | JSON Schema files (not deployed, embedded at compile time) |

`~/.ai/tools/bin/` is on `$PATH`. All binaries are discoverable by name.
