# Nornir Context Map

**Generated:** 2026-03-20
**Purpose:** Validation-as-code Rust monorepo. Produces compiled binaries, PyO3 gate modules, and libraries for the `.ai` ecosystem. Schemas embedded at compile time. Three-tier architecture: core (pure) -> capability (I/O) -> binaries.

---

---

## 0. Start here — the pipeline orientation set

Before this map, read the orientation documents at
`~/.ai/smidja/galdr/context/`. They carry the reasoning behind the whole
Verdandi → Draupnir → Nornir → Regin → Galdr pipeline — **why it exists, what each stage
buys, and which parts are not built** — which is the one thing that cannot be recovered by
reading a tree. Every previous session that skipped them inferred a purpose from the
structure and got it wrong.

- `context/AGENT_BUILD_SYSTEM.md` — the map. Read first.
- `context/NORNIR_GATES_ELEMENT.md` — this project's part. Read second.

They are wikilinked to each other; follow them as needed.

## Primary References

| Document | When to read | Freshness |
|----------|-------------|-----------|
| `NORNIR_CONVENTIONS.md` | Before writing ANY code | CURRENT — updated 2026-03-19 |
| `CLAUDE.md` | Auto-loaded every session | CURRENT — restructured 2026-03-13 as Layer 2 |
| `MUST_READ_BEFORE_BUILDING.md` | Before ANY build/deploy operation | CURRENT — nornir_deploy CLI + verify methods reference |
| `audit/STRUCTURAL_AUDIT_GUIDE.md` | When auditing or reviewing code | CURRENT — priorities P1-P10 match codebase |
| `hooks/HOOK_DESIGN.md` | When working on hooks | CURRENT — updated 2026-03-19 |
| `cli/syn_cli/SYN_DESIGN.md` | When working on syn/saga quality pipeline | CURRENT — updated 2026-03-19 |
| `interceptors/INTERCEPT_DESIGN.md` | When working on intercept pipeline | CURRENT — created 2026-03-19 |

## Documentation References (All)

| Document | Status | Notes |
|----------|--------|-------|
| `NORNIR_CONVENTIONS.md` | CURRENT | Updated 2026-03-19: deploy refs, test command, workspace deps |
| `CLAUDE.md` | CURRENT | Session entry point, Layer 2 context protocol |
| `CONTEXT_MAP.md` | CURRENT | This file — regenerate after significant changes |
| `MUST_READ_BEFORE_BUILDING.md` | CURRENT | nornir_deploy usage, verify methods, build process guidance |
| `audit/STRUCTURAL_AUDIT_GUIDE.md` | CURRENT | Architectural invariants P1-P10 |
| `hooks/HOOK_DESIGN.md` | CURRENT | Updated 2026-03-19: Severity, HookDecision, dispatch, wire format |
| `cli/syn_cli/SYN_DESIGN.md` | CURRENT | Updated 2026-03-19: PostToolUse uses report mode |
| `interceptors/INTERCEPT_DESIGN.md` | CURRENT | Created 2026-03-19: intercept pipeline architecture |
| `plans/IMPROVEMENT_PLAN.md` | ACTIVE | 10 prioritized items from dual audit |

---

## Crate Inventory

**97 workspace members:** 16 core + 11 capability + 35 gates + 8 checks + 4 tools + 5 writers + 6 hooks + 5 senders + 1 rewriter + 1 converter + 1 dispatcher + 1 watcher + 2 interceptors + 1 daemon.

### Tier 1: Core (16 crates, pure, no I/O)

| Crate | Purpose |
|-------|---------|
| `announce_core` | Voice/TTS config resolution, hash, sanitize, PCM conversion |
| `compaction_inject_core` | Compaction summary instructions injection |
| `datagram_core` | Datagram, DatagramKind, Priority type definitions |
| `default_apply_core` | Default value application logic |
| `diff_core` | Exchange diffing, TOML block extraction, pace/workspace/accumulate utils |
| `error_core` | ValidationIssue types, educational error formatting |
| `format_core` | JSON/YAML/TOML/TOON/TOMLX conversion |
| `gleipnir_core` | Tree-sitter AST guardrail engine |
| `intercept_core` | Exchange classification: ExchangeKind, classify_exchange, has_tool |
| `path_core` | Path field extraction, validate_path_segment |
| `report_render_core` | QA report grouping, formatting, serialization |
| `saga_core` | SanityReport + Issue types, pure path functions |
| `schema_core` | EmbeddedValidator with lazy-static schema loading |
| `syn_core` | Jq filter compilation, three-tier filtering, SynConfig |
| `text_core` | Markdown stripping for TTS and display |
| `time_core` | civil_date(), iso_zulu() — pure epoch-to-string conversions |

### Tier 2: Capability (11 crates)

| Crate | Purpose |
|-------|---------|
| `schemas_embedded` | All schemas via include_str!() |
| `path_verify_io` | Filesystem path existence checks |
| `io_check` | File-arg diagnostic output contract |
| `gate_io` | Gate I/O orchestration (read/validate/write) |
| `hook_io` | Hook input parsing, response formatting, rules, make_decision |
| `datagram_io` | Dual-transport datagram emission |
| `intercept_io` | PyO3: json_to_toml + append_jsonl_line for bifrost |
| `write_engine` | Config-driven atomic writes, fsync, ai_home() |
| `saga_runner` | QA report generation, directory walker, sidecar I/O |
| `default_apply_io` | Default value application with file I/O |
| `session_io` | Shared session file I/O: append_exchange/subagent/compaction, record_compaction |

### Tier 3: Binaries (35 cargo executables + 35 gate modules + 2 PyO3 modules)

- **35 gate modules** in `gates/` — PyO3 pipeline stage validators
- **8 check CLIs** in `cli/` — `check_raw_definition` through `check_anthropic_render`
- **4 specialist tools** in `cli/` — `saga_cli` (binary: saga), `syn_cli` (binary: syn), `hush`, `announce`
- **5 writers** in `writers/` — `append_truth_qc_report_record`, `append_interview_summaries_record`, `append_embedding_normalize_batch_20`, `append_raw_jsonl`, `write_truth_glossary_record`
- **6 hooks** in `hooks/` — `hook_pre_llm_tool`, `hook_pre_llm_bash`, `hook_pre_subagent_tool`, `hook_pre_subagent_bash`, `hook_post_llm_tool`, `hook_stop_llm_tts`
- **5 senders** in `senders/` — `send_alert`, `send_warning`, `send_notification`, `send_heartbeat`, `send_datagram`
- **1 rewriter** — `rewrite_compaction_summary`
- **1 converter** — `convert_json_to_toml`
- **1 dispatcher** — `split_jsonl_batches`
- **1 watcher** — `watch_and_diff_exchange_intercepts`
- **2 interceptors** — `traffic_interceptor_rewriter` (PyO3 module), `intercept_replay` (binary)
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
| What architectural violations look like | `audit/STRUCTURAL_AUDIT_GUIDE.md` |
| What a writer binary looks like | `NORNIR_CONVENTIONS.md` > Composition Patterns > Writers |
| How to add a new crate | `NORNIR_CONVENTIONS.md` > Adding a New Crate |
| How the intercept pipeline works | `interceptors/INTERCEPT_DESIGN.md` |
| format_core TOMLX path expansion | `core/format_core/src/tomlx/mod.rs` — pass `resolve_env` closure |

---

## Known Issues / Active Work

- `traffic_interceptor_rewriter` naming exception — should be `intercept_traffic_rewrite` per conventions. Rename deferred.
- `schemas_embedded` lives in capability/ but has no I/O — could be core/. Move deferred.
- `announce` naming exception — no verb prefix. Decomposed (item 1 done) and moved to cli/.
- `hush` naming exception — no verb prefix, lives in cli/ but is a UserPromptSubmit hook.
- Hook input wire format has no JSON Schema — typed accessors exist but no `.schema.json` file.
- Syn config (`.syn/warn.toml`, `.syn/deny.toml`) parsed with typed structs but no schema file.
- Ratchet comparison engine for syn gate mode not yet built (SYN_DESIGN.md Phase 2).
