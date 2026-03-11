# Nornir Quickstart

## What Nornir Does

Nornir is a Rust monorepo workspace at `~/.ai/smidja/nornir/`. It produces validated binaries, Python extension modules, and libraries that form the tooling infrastructure for the entire `.ai` ecosystem.

A **validation-as-code** system. Schemas are the source of truth, embedded in binaries at compile time via `include_str!()`. Invalid data never reaches processing logic.

Nornir produces:
- **8 CLI check binaries** (`check_*`) — validate TOML files against schemas
- **33 PyO3 gate modules** (`gate_*`) — Python-importable Rust validation
- **5 writer binaries** (`append_*`, `write_*`) — schema-validated output with fsync
- **5 hook binaries** (`hook_*`) — LLM security interceptors
- **5 sender binaries** (`send_*`) — datagram emitters to Hlidskjalf
- **1 rewriter binary** (`rewrite_*`) — JSON request transformation
- **1 converter binary** (`convert_*`) — format conversion
- **1 dispatcher binary** (`split_*`) — batch processing
- **2 specialist tools** (`saga`, `syn`) — quality pipeline

---

## Building and Deploying

Every binary category has a deploy script. Do NOT use bare `cargo build`.

```bash
cd /Users/johnny/.ai/smidja/nornir

./deploy_gates.py         # CLI check tools + PyO3 gate modules
./deploy_hooks.py         # Hook binaries
./deploy_writers.py       # Writer binaries
./deploy_rewriters.py     # Rewriter binaries
./deploy_senders.py       # Sender binaries
./deploy_converters.py    # Converter binaries
./deploy_watchers.py      # Watcher binaries
./deploy_dispatchers.py   # Dispatcher binaries
./deploy_tools.py         # Specialist tools (saga, syn)
```

Deploy scripts build release binaries, create symlinks in `~/.ai/tools/bin/`, and verify.

---

## Architecture

Rust workspace with 78 member crates in three tiers:

```
Tier 1: CORE (10 pure libraries, no I/O)
    error_core, format_core, schema_core, path_core,
    saga_core, gleipnir_core, diff_core, datagram_types,
    report_render_core, compaction_inject_core

Tier 2: CAPABILITY (10 feature libraries, may have I/O)
    schemas_embedded, path_verify, io_filter, io_check,
    gate_io, hook_io, datagram, intercept_io,
    write_core, saga_runner

Tier 3: BINARIES (62 executables and Python extensions)
    gates/*, cli/*, writers/*, hooks/*, senders/*,
    converters/*, rewriters/*, watchers/*, dispatchers/*
```

**Rules:** Core depends only on core. Capability depends on core + capability. Binaries depend on core + capability, never on other binaries.

---

## Quality Pipeline (saga + syn)

```
saga (truth)  →  .qa JSON  →  syn (policy)  →  filtered output + decision
```

**saga** runs quality tools (gleipnir, ruff, basedpyright) on Python files and writes `.qa` sidecar files. Raw truth, no filtering.

**syn** reads `.qa` truth and applies policy — filtering via jq expressions, grouping by tool+code, formatting (TOON/colored/JSON), broadcasting to Hlidskjalf.

```bash
# Generate .qa sidecars for a project
saga /path/to/project

# View quality report (default: gleipnir issues only)
syn /path/to/project

# Show all tools, error severity and above
syn --tool all --level error /path/to/project

# Machine-readable JSON
syn --json /path/to/project

# Gate mode (deterministic, for hooks)
syn --mode gate /path/to/project
```

---

## Hooks (LLM Security)

Five hooks gate LLM actions:

| Hook | Event | Detects |
|------|-------|---------|
| `hook_pre_llm_tool` | PreToolUse (Read/Write/Edit) | Probing security infrastructure, gaming constraints |
| `hook_pre_llm_bash` | PreToolUse (Bash) | Lock subversion, constraint truncation, evasion |
| `hook_pre_subagent_tool` | PreToolUse (subagent) | Path escape from allowed prefixes |
| `hook_pre_subagent_bash` | PreToolUse (subagent) | Shell chaining, unauthorized commands |
| `hook_post_llm_tool` | PostToolUse (Write/Edit) | Quality assessment pipeline (saga → syn) |

Rules are embedded TOML parsed at startup. Three severity layers: floor (always block), configurable categories (warn or block), allow (everything else). Fail-closed on config errors.

---

## Writers

Declarative ~16-line binaries. Define config, call `write_core::run()`:

```bash
# Append a record (schema-validated, fsync'd)
echo '{"uid":"abc","assessment":"..."}' | append_truth_qc_report_record

# Raw JSON append to traffic directory
echo '{"key":"value"}' | append_raw_jsonl traffic-log
```

---

## Senders (Hlidskjalf Datagrams)

Fire-and-forget messages to the Hlidskjalf Unix socket via `datagram`:

```bash
send_alert --source saga --detail "Quality regression detected"
send_notification --source syn --detail "All checks passed"
send_heartbeat --source hook_post_llm_tool
```

---

## CLI Check Tools

Validate TOML agent definitions against schemas:

```bash
check_raw_definition my-agent.toml          # File argument
cat my-agent.toml | check_paths_verified    # Stdin
check_universal_format my-agent.toml --json # JSON output
```

Exit codes: 0 = valid, 1 = invalid, 2 = operational error.

---

## Where Things Live

| Resource | Path |
|---|---|
| Nornir workspace | `~/.ai/smidja/nornir/` |
| Schemas (source of truth) | `nornir/schemas/` |
| Deploy scripts | `nornir/deploy_*.py` |
| Built binaries | `nornir/target/release/` |
| Deployed symlinks | `~/.ai/tools/bin/` |
| Deployed gate modules | `~/.ai/tools/lib/` |

---

## Test Coverage

576 tests across 21 crates, all passing. Security-critical hooks have dual-direction testing: every detection rule verified for true positives AND true negatives.

Run tests: `cargo test -p {crate_name}` (avoid full workspace `cargo test` due to PyO3 gate linker requirements).

---

## Key Design Principles

- **Schemas are the single source of truth** — never duplicate validation in code
- **Educational errors** — what/where/found/expected/fix
- **Compile-time embedding** — schemas baked in, no runtime file dependencies
- **Result-based control flow** — helpers return Result, only main calls process::exit
- **Pure/impure separation** — core crates have no I/O
- **Atomic writes** — temp file + fsync + rename for durability
- **Fail-closed security** — config parse errors deny, never allow
