# Strict Architectural Audit A

**Auditor:** A
**Date:** 2026-03-20
**Scope:** All 95 workspace members, checked against STRUCTURAL_AUDIT_GUIDE.md invariants P1-P10.

---

## P1: Pure Logic Must Not Live in Binary Crates

### VIOLATION: `watch_and_diff_exchange_intercepts` — transcript I/O functions (498 lines)

`transcript_path_for()`, `open_transcript()`, and `append_transcript()` at lines 330-356 are session-file I/O functions that duplicate responsibilities already handled by `session_io`. The watcher independently manages `running_transcript.jsonl` with its own open/append/path-derivation logic, while `session_io` is the designated capability crate for session file I/O. These functions should either be moved into `session_io` or the watcher should delegate to it.

Additionally, `jitter_sleep()` (lines 60-67) is a pure PRNG function with no I/O dependency — it takes seed/min/max and computes a delay. While small, it is pure logic in a binary.

**Severity:** Medium. The transcript functions are the real concern — a second consumer of `running_transcript.jsonl` would need to reimplement this path derivation.

### VIOLATION: `split_jsonl_batches` — `compute_batches()` is pure algorithm (388 lines)

`compute_batches()` at lines 70-98 is a pure batch-sizing algorithm (takes total/min/max, returns Vec<usize>). It has no I/O, no side effects, and is well-tested with 10 tests. This is textbook extractable logic. If any future tool needs optimal batch splitting, it will reimplement this differently.

**Severity:** Low. Single consumer currently, but the function is clearly pure.

### OBSERVATION: `hook_pre_llm_bash` (1180 lines) and `hook_pre_subagent_bash` (820 lines)

These are large but structurally correct. The decision logic (`decide`, `check_category`) is pure — takes input, returns HookDecision, no I/O. Rule parsing delegates to `hook_io::rules`. Config parsing reads `std::env::args` and env vars, which is acceptable for binary-tier config resolution. The rule structs (`CompiledRule`, `Rules`, `Config`) are specific to this hook's categories and do not generalize.

The `hook_pre_subagent_bash` helper functions (`has_chain_chars`, `is_bare_name`, `parse_heredoc_header`, `validate_writer`, `validate_inspect`) are security-specific pure functions. An argument could be made for extracting the shell-safety functions (`has_chain_chars`) to a core crate if other hooks need them, but currently only this hook uses them.

**No violation — architecture is correct despite size.**

### OBSERVATION: `announce` (460 lines)

Large but correctly structured. Pure logic is in `announce_core` (585 lines with 27 tests). The binary handles only I/O: config loading, API calls, audio playback, process spawning. No pure logic leaks.

---

## P2: Three-Tier Dependency Model

### NO VIOLATIONS FOUND

All core crates depend only on other core crates and workspace external deps:
- `default_apply_core` -> `error_core` (core-to-core)
- `diff_core` -> `datagram_core` (core-to-core)
- `format_core` -> `error_core` (core-to-core)
- `path_core` -> `error_core` (core-to-core)
- `report_render_core` -> `saga_core`, `format_core` (core-to-core)
- `schema_core` -> `error_core` (core-to-core)
- `syn_core` -> `saga_core`, `report_render_core` (core-to-core)

All capability crates depend on core + capability only. No binary-to-binary dependencies detected.

No core crate performs I/O. The `format_core` comments mention `std::env::var` but only in documentation examples showing how callers should pass a closure — the core crate itself takes `resolve_env: impl Fn(&str) -> Option<String>` as a parameter, keeping it pure.

### KNOWN EXCEPTIONS (documented in CONTEXT_MAP.md)

- `schemas_embedded` lives in `capability/` but has no I/O. Could be `core/`. Move deferred.
- `datagram_core` naming: should be `datagram_types` per convention (types-only crate), but `_core` suffix is acceptable for core tier.

---

## P3: process::exit and Panic Discipline

### NO VIOLATIONS FOUND

All `process::exit()` calls are inside `main()` functions. Verified across all 35+ binary crates. All helpers return `Result`.

Hook binaries use `fn main() -> ExitCode` with `hook_io::run_hook(decide)` / `hook_io::run_post_hook(assess)` — the hook_io framework handles exit codes, and the binary's `decide`/`assess` functions return `HookDecision` / `Option<String>`, not exits.

### OBSERVATION: `.unwrap()` in static initializers

`hook_pre_subagent_bash` uses `.unwrap()` in `LazyLock<Regex>` initializers (lines 22, 26, 30). This is the documented exemption. No `.unwrap()` or `.expect()` found in non-static production code paths.

---

## P4: Composition Over Reimplementation

### VIOLATION: `watch_and_diff_exchange_intercepts` reimplements session transcript I/O

As noted in P1, `transcript_path_for()` derives the path to `running_transcript.jsonl` as a sibling of the main exchange log. The `session_io` crate handles the same session directory structure. The watcher should use `session_io` for transcript path derivation rather than reimplementing the convention.

### VIOLATION: `intercept_replay` — `append_raw` reimplements raw log appending

The `traffic_interceptor_rewriter` at line 35-51 has its own `append_raw()` that does `OpenOptions::new().create(true).append(true).open()` + write + flush. This is the same pattern as `write_engine::append_line_fsync()` but without fsync. The interceptor should use `write_engine` for consistency, or `session_io` should expose a raw-append function.

**Severity:** Low — the interceptor intentionally skips fsync for performance (raw capture is fire-and-forget), so this may be a deliberate tradeoff rather than an oversight. But the divergence should be documented.

### OBSERVATION: `convert_json_to_toml` correctly composes

Uses `format_core::convert::strip_nulls` and `format_core::serialize::to_toml` for conversion, `write_engine::write_file_atomic` for file writing. Correct composition.

---

## P5: Naming Encodes Architecture

### KNOWN EXCEPTIONS (documented)

1. **`traffic_interceptor_rewriter`** — should be `intercept_traffic_rewrite` per conventions. Rename deferred.
2. **`announce`** — no verb prefix. Lives in `cli/` but is a TTS tool, not a check binary.
3. **`hush`** — no verb prefix. Lives in `cli/` but is a UserPromptSubmit hook.
4. **`datagram_core`** — should arguably be `datagram_types` since it's only types, but `_core` is acceptable.

### VIOLATION: `senders/announce/` — orphaned directory

`git status` shows `senders/announce/` as an untracked directory. The `announce` crate has been moved to `cli/announce/` (it's in the workspace Cargo.toml as `cli/announce`). The `senders/announce/` directory appears to be a leftover from a move operation. This is an orphaned artifact.

### NO NEW VIOLATIONS

All other crates follow the identity rule (directory name = package name = binary name). Specialist tools `saga_cli` -> `saga` and `syn_cli` -> `syn` are properly documented exceptions with `binary_names` mapping in `deploy_categories.toml`.

---

## P6: Security Hook Coverage

### STRONG: `hook_pre_llm_bash` — 72 tests

Excellent dual-direction coverage:
- Subversion: 6 detection tests + benign tests (ls, cargo build, cat, git status, git diff)
- Truncation: 6 detection tests for head/tail/grep/sed/awk/pipe patterns
- Evasion: 5 detection tests (git checkout/restore/config, mv, cp)
- Destruction: 7 detection tests + 3 benign tests (git reset --soft, checkout -b, clean -n)
- Revert: 5 detection tests + 5 benign tests (--staged, stash list/pop/show, checkout branch)
- Workflow: 5 detection tests + 3 benign tests (cargo test/check/clippy)
- Severity mapping, exemption, and truncation boundary tests

### STRONG: `hook_pre_subagent_bash` — 35 tests

Full coverage of writer validation, inspect validation, shell chaining, path traversal, heredoc parsing, and the allow/deny decision tree.

### STRONG: `hook_pre_llm_tool` — 28 tests

Floor (always-block), probing, gaming, and allow_paths tested with all severity combinations. Per-rule severity override tested (settings uses ask).

### STRONG: `hook_pre_subagent_tool` — 16 tests

Path prefix validation, unknown tool denial, path traversal, file_path vs path field precedence.

### WEAKNESS: `hook_stop_llm_tts` — 0 tests

This hook has zero tests. While it is not a security hook (it's a stop hook for TTS playback), it contains decision logic: checking QUIET.lock and SILENT.lock files, filtering markdown via `text_core`, and spawning the announce process. The markdown filtering is tested in `text_core`, but the hook's own decision flow is untested.

### WEAKNESS: `hook_post_llm_tool` — 11 tests (limited)

Tests cover `classify_file` and `extract_file_path` but not the `assess_source` function (which spawns saga + syn). This is understandable since `assess_source` requires external binaries, but the gap should be acknowledged.

---

## P7: Schema-First Data Validation

### NO VIOLATIONS FOUND

- Gates use `schemas_embedded` validators. 27 schema constants embedded.
- Writers use `write_engine::run()` with schema reference.
- `traffic_interceptor_rewriter` and `intercept_replay` embed `cc_wire_schema.json` via `include_str!()` and validate with `EmbeddedValidator`.
- Datagram emission uses `emit_validated()` or `emit_validated_or_alert()` which validate against the datagram schema.
- No procedural shape-checking found in binary crates — classification logic in `intercept_core` uses field presence checks (`has_tool`, `tool_count`) but these are semantic classification, not schema validation substitutes.

### KNOWN GAPS (documented)

- Hook input wire format has no JSON Schema — typed accessors exist in `hook_io` but no `.schema.json` file.
- Syn config (`.syn/warn.toml`, `.syn/deny.toml`) parsed with typed structs but no schema file.

---

## P8: Stale Tests After Contract Changes

### NO VIOLATIONS DETECTED

Tests appear current with their contracts. Specifically checked:
- `hook_pre_llm_bash` tests match the 6 rule categories (subversion, truncation, evasion, destruction, revert, workflow)
- `intercept_core` tests match the current `ExchangeKind` enum (Main, Subagent, Compaction)
- `announce_core` tests match the current `resolve_settings` 4-layer merge (CLI > VOICE.lock > profile > default)
- `compaction_inject_core` tests verify current injection contract (system array, text type)

---

## P9: Gleipnir Check Accuracy

### NOT IN SCOPE

Per the audit guide, gleipnir check accuracy requires running gleipnir and analyzing its output. This audit reads code, not tool output. Deferring to gleipnir's own self-check infrastructure.

### OBSERVATION: Known deferred violations

MEMORY.md documents: `report_render_core` has 3x clone() violations and 1x String::from violation at lines 47, 330, 396. These are known and deferred.

---

## P10: Orphaned Artifacts

### VIOLATION: `senders/announce/` — orphaned directory

Untracked directory visible in `git status`. The `announce` crate now lives at `cli/announce/`. The `senders/announce/` directory should be deleted.

### NO OTHER ORPHANS DETECTED

- All workspace Cargo.toml members point to existing crate directories.
- All `deploy_categories.toml` crate entries match workspace members.
- `intercept_replay/cc_wire_schema.json` is a symlink to `traffic_interceptor_rewriter/cc_wire_schema.json` — both exist.
- No dead entries in deploy script crate lists.

### OBSERVATION: Zero-test library crates

The following library crates have zero tests:
- `capability/default_apply_io` (73 lines)
- `capability/intercept_io` (54 lines)
- `capability/saga_runner` (545 lines — significant)
- `core/datagram_core` (47 lines — types only, acceptable)
- `core/format_core` (23 lines lib.rs — re-exports, acceptable)

`saga_runner` at 545 lines with zero tests is the most concerning. It handles QA report generation, directory walking, sidecar I/O, and orphan detection. These are tested indirectly through `saga_cli` and `syn_cli`, but the capability crate itself lacks unit tests.

---

## Summary of Findings

| Priority | Finding | Severity |
|----------|---------|----------|
| P1 | `watch_and_diff_exchange_intercepts` transcript I/O should be in `session_io` | Medium |
| P1 | `split_jsonl_batches` `compute_batches()` is extractable pure logic | Low |
| P4 | `traffic_interceptor_rewriter` `append_raw` reimplements append without fsync | Low |
| P5 | `senders/announce/` orphaned directory from crate move | Low |
| P6 | `hook_stop_llm_tts` has zero tests | Medium |
| P10 | `senders/announce/` orphaned directory (same as P5) | Low |
| P10 | `saga_runner` has 545 lines and zero tests | Medium |

### Items confirmed as NOT violations (known exceptions)

- `traffic_interceptor_rewriter` naming — documented, deferred
- `announce` naming — documented exception
- `hush` naming — documented exception
- `schemas_embedded` placement — documented, deferred
- `datagram_core` naming — acceptable
- `report_render_core` clone/String violations — documented, deferred

### Architecture health assessment

The codebase is in strong shape. The three-tier model is respected with zero tier violations. process::exit discipline is perfect. Security hooks have exemplary dual-direction test coverage. Schema-first validation is consistently applied. The violations found are minor — accumulated drift at the edges rather than structural damage. The most actionable items are the `saga_runner` test gap and the watcher transcript I/O extraction.
