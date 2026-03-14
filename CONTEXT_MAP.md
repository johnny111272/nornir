# Nornir Context Map

**Generated:** 2026-03-13
**Purpose:** Validation-as-code Rust monorepo. Produces compiled binaries, PyO3 gate modules, and libraries for the `.ai` ecosystem. Schemas embedded at compile time. Three-tier architecture: core (pure) -> capability (I/O) -> binaries.

---

## Primary References

| Document | When to read | Freshness |
|----------|-------------|-----------|
| `NORNIR_CONVENTIONS.md` | Before writing ANY code | CURRENT — consolidated 2026-03-13 from naming/org/building docs |
| `CLAUDE.md` | Auto-loaded every session | CURRENT — restructured 2026-03-13 as Layer 2 |
| `MUST_READ_BEFORE_BUILDING.md` | Before ANY build/deploy operation | CURRENT — nornir_deploy CLI reference |
| `audit/AUDIT_GUIDE.md` | When auditing or reviewing code | CURRENT — priorities P1-P10 match codebase |
| `hooks/HOOK_DESIGN.md` | When working on hooks | CURRENT — wire format, dispatch architecture, runner separation |
| `cli/syn_cli/SYN_DESIGN.md` | When working on syn/saga quality pipeline | MOSTLY CURRENT — Phase 1 done, ratchet comparison not yet built |

## Documentation References (All)

| Document | Status | Notes |
|----------|--------|-------|
| `NORNIR_CONVENTIONS.md` | CURRENT | Single source of truth for conventions |
| `CLAUDE.md` | CURRENT | Session entry point, Layer 2 context protocol |
| `CONTEXT_MAP.md` | CURRENT | This file — regenerate after significant changes |
| `MUST_READ_BEFORE_BUILDING.md` | CURRENT | nornir_deploy usage, build process guidance |
| `audit/AUDIT_GUIDE.md` | CURRENT | Architectural invariants P1-P10 |
| `hooks/HOOK_DESIGN.md` | CURRENT | Hook subsystem design |
| `cli/syn_cli/SYN_DESIGN.md` | MOSTLY CURRENT | syn_core extraction done, line counts updated |
| `plans/IMPROVEMENT_PLAN.md` | COMPLETE | All 10 items done. Historical reference only. |
| `NORNIR_NAMING.md` | SUPERSEDED | Replaced by NORNIR_CONVENTIONS.md |
| `NORNIR_ORGANIZATION.md` | SUPERSEDED | Replaced by NORNIR_CONVENTIONS.md |
| `NORNIR_BUILDING_AND_COMPOSITION.md` | SUPERSEDED | Replaced by NORNIR_CONVENTIONS.md |
| `MANDATORY_READ_BEFORE_CODING.md` | SUPERSEDED | Absorbed into CLAUDE.md |
| `QUICKSTART.md` | SUPERSEDED | Content covered by CONVENTIONS + CONTEXT_MAP |

---

## Crate Inventory

**85 workspace members:** 11 core + 10 capability + 33 gates + 8 checks + 2 tools + 5 writers + 5 hooks + 5 senders + 1 rewriter + 1 converter + 1 dispatcher + 1 watcher + 1 interceptor + 1 daemon.

### Tier 1: Core (11 crates, pure, no I/O)

| Crate | Purpose | Tests |
|-------|---------|-------|
| `error_core` | ValidationIssue types, educational error formatting | 10 |
| `format_core` | JSON/YAML/TOML/TOON/TOMLX conversion | 78 |
| `schema_core` | EmbeddedValidator with lazy-static schema loading | 3 |
| `path_core` | Path field extraction, validate_path_segment | 14 |
| `saga_core` | SanityReport + Issue types, pure path functions | 6 |
| `syn_core` | Jq filter compilation, three-tier filtering, SynConfig | 39 |
| `gleipnir_core` | Tree-sitter AST guardrail engine | 267 |
| `diff_core` | Line-level diff and TOML block extraction | 26 |
| `datagram_types` | Datagram, DatagramKind, Priority type definitions | 17 |
| `report_render_core` | QA report grouping, formatting, serialization | 38 |
| `compaction_inject_core` | Compaction summary instructions injection | 7 |

### Tier 2: Capability (10 crates)

| Crate | Purpose | Tests |
|-------|---------|-------|
| `schemas_embedded` | All schemas via include_str!() | 9 |
| `path_verify_io` | Filesystem path existence checks | 8 |
| `io_filter` | stdin-validate-stdout filter contract | 0 |
| `io_check` | File-arg diagnostic output contract | 13 |
| `gate_io` | Gate I/O orchestration (read/validate/write) | 0 |
| `hook_io` | Hook input parsing, response formatting, rules | 40 |
| `datagram_io` | Dual-transport datagram emission | 0 |
| `intercept_io` | PyO3: json_to_toml + append_jsonl_line for bifrost | (PyO3) |
| `write_engine` | Config-driven atomic writes, fsync, ai_home() | 12 |
| `saga_runner` | QA report generation, directory walker, sidecar I/O | 29 |

### Tier 3: Binaries (64 executables + 33 gate modules)

- **33 gate modules** in `gates/` — PyO3 pipeline stage validators
- **8 check CLIs** in `cli/` — `check_raw_definition` through `check_anthropic_render`
- **2 specialist tools** in `cli/` — `saga_cli` (binary: saga), `syn_cli` (binary: syn)
- **5 writers** in `writers/` — `append_truth_qc_report_record`, `append_interview_summaries_record`, `append_embedding_normalize_batch_20`, `append_raw_jsonl`, `write_truth_glossary_record`
- **5 hooks** in `hooks/` — `hook_pre_llm_tool`, `hook_pre_llm_bash`, `hook_pre_subagent_tool`, `hook_pre_subagent_bash`, `hook_post_llm_tool`
- **5 senders** in `senders/` — `send_alert`, `send_warning`, `send_notification`, `send_heartbeat`, `send_datagram`
- **1 rewriter** — `rewrite_compaction_summary`
- **1 converter** — `convert_json_to_toml`
- **1 dispatcher** — `split_jsonl_batches`
- **1 watcher** — `watch_and_diff_exchange_intercepts`
- **1 interceptor** — `traffic_interceptor_rewriter`
- **1 daemon** — `record_datagrams`

---

## Context Refresh Guide

If you need to understand... read...

| Topic | Source |
|-------|--------|
| How to name anything | `NORNIR_CONVENTIONS.md` > Naming section |
| Which tier a crate belongs in | `NORNIR_CONVENTIONS.md` > Architecture section |
| How to structure a binary | `NORNIR_CONVENTIONS.md` > Binary Structure section |
| What crate to import for a task | `NORNIR_CONVENTIONS.md` > Dependency Lookup table |
| How to deploy | `MUST_READ_BEFORE_BUILDING.md` + `NORNIR_CONVENTIONS.md` > Deploying section |
| How hooks work (wire format, dispatch) | `hooks/HOOK_DESIGN.md` |
| How syn/saga quality pipeline works | `cli/syn_cli/SYN_DESIGN.md` |
| What architectural violations look like | `audit/AUDIT_GUIDE.md` |
| What a writer binary looks like | `NORNIR_CONVENTIONS.md` > Composition Patterns > Writers |
| How to add a new crate | `NORNIR_CONVENTIONS.md` > Adding a New Crate |
| format_core TOMLX path expansion | `core/format_core/src/tomlx/mod.rs` — pass `resolve_env` closure |

---

## Known Issues / Active Work

- `io_filter` crate is orphaned — imported by nothing, zero tests. Candidate for removal or integration.
- `traffic_interceptor_rewriter` naming exception — should be `intercept_traffic_rewrite` per conventions. Rename deferred.
- `datagram_types` naming exception — should be `datagram_core` per core crate suffix convention. Rename deferred.
- `schemas_embedded` lives in capability/ but has no I/O — could be core/. Move deferred.
- Hook input wire format has no JSON Schema — typed accessors exist but no `.schema.json` file.
- Syn config (`.syn/warn.toml`, `.syn/deny.toml`) parsed with typed structs but no schema file.
- Ratchet comparison engine for syn gate mode not yet built (SYN_DESIGN.md Phase 2).

---

## Test Summary

877 tests across workspace, 0 failures (2026-03-13). Gate crates excluded from workspace test runs (PyO3 linker requirements — use `nornir_deploy --build gates`).
