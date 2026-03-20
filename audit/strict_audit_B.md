# Strict Architectural Audit B

**Auditor:** B
**Date:** 2026-03-20
**Scope:** Full workspace audit against STRUCTURAL_AUDIT_GUIDE.md invariants P1-P10

---

## P1: Pure Logic Must Not Live in Binary Crates

### VIOLATION: `watch_and_diff_exchange_intercepts` — transcript I/O reimplemented (498 lines)

The watcher binary implements its own `open_transcript()`, `append_transcript()`, and `transcript_path_for()` functions (lines 330-356) using raw `OpenOptions` and `file.write_all()`. This is JSONL append with no fsync — the same operation `write_engine::append_line_fsync` handles correctly. The binary should use `write_engine` for transcript writes, or at minimum `session_io` which already handles session file I/O.

Additionally, the `jitter_sleep()` PRNG function (lines 60-67) is pure deterministic logic (xorshift64) trapped in a binary. If any future tool needs jittered delays, this will be reimplemented.

**Severity:** Medium. The transcript writing diverges from write_engine's fsync discipline — data can be lost on crash. The jitter function is low risk but still extractable.

### VIOLATION: `split_jsonl_batches` — `compute_batches()` is pure extractable logic (388 lines)

The `compute_batches()` function (lines 70-98) is pure: takes `(total, min_batch, max_batch)`, returns `Vec<usize>`. No I/O, no side effects. This is a batch-sizing algorithm that any future dispatch tool could need. It belongs in a core crate. The function has thorough tests (lines 218-306) which confirms it is standalone reusable logic.

**Severity:** Low. Single consumer today, but the function is cleanly extractable.

### VIOLATION: `announce` — 460 lines with significant I/O orchestration

The `announce` binary at 460 lines is the largest binary outside the security hooks. While it properly delegates pure logic to `announce_core`, the binary contains substantial I/O orchestration: HTTP client calls, file caching, hit-count management, audio playback, symlink management, and process forking. This is within acceptable bounds for a complex tool, but the `ensure_voice_symlinks()` function (lines 269-285) and the hit-counting logic (`load_hits`/`save_hits`, lines 252-263) are patterns that could accumulate if another TTS-related binary appears.

**Severity:** Low. Mostly orchestration, not pure logic. The `announce_core` extraction was done correctly.

### OBSERVATION: `hook_pre_llm_bash` (1180 lines) and `hook_pre_subagent_bash` (820 lines)

These are the largest binaries. However, the size is predominantly tests (900+ lines of tests in hook_pre_llm_bash, 500+ in hook_pre_subagent_bash). The actual logic is thin: `parse_rules()` uses `hook_io::rules`, `check_category()` is ~20 lines of matching, `decide()` is orchestration. The `CompiledRule` struct and `from_raw()` method are reasonable binary-local types. No P1 violation — the architecture is correct.

---

## P2: The Three-Tier Dependency Model

### NO VIOLATIONS FOUND

All dependency paths are correct:
- Core crates depend only on other core crates and workspace external deps.
- Capability crates depend on core and other capability crates.
- No binary-to-binary dependencies detected.
- No core crate performs I/O (verified by scanning all `core/*/src/lib.rs` for `std::fs`, `std::env`, `std::net`, `std::io`, `std::process` — zero hits).

### KNOWN ISSUE: `schemas_embedded` in `capability/` has no I/O

`schemas_embedded` uses only `include_str!()` (compile-time) and `schema_core` (a core crate). It performs no runtime I/O. It could be a core crate. Documented as known issue in CONTEXT_MAP.md — deferral is acknowledged.

---

## P3: process::exit and Panic Discipline

### VIOLATION: `intercept_replay` — `process::exit()` in `main()` but scattered across multiple early-return points

The `intercept_replay` binary has 4 `process::exit(1)` calls in `main()` (lines 152, 157, 164, 180). While technically all in `main()`, the pattern is fragile: each step has its own error-handling block with `process::exit()` rather than using the standard `parse_args/run/match` pattern. If helper logic is ever added between these calls, the exit discipline is easy to break. Compare with the clean pattern in `rewrite_compaction_summary` or `record_datagrams`.

**Severity:** Low. All exits are in `main()` so no invariant is broken, but the structure invites future drift.

### OBSERVATION: All helper functions across all binaries return `Result`

Verified across all binary crates. No `process::exit()` found outside `main()` functions. `.unwrap()` usage is limited to `LazyLock` initializers (`hook_pre_subagent_bash` lines 22-31) and test code, which are the documented exemptions.

---

## P4: Composition Over Reimplementation

### VIOLATION: `watch_and_diff_exchange_intercepts` — custom JSONL append without write_engine

As noted in P1, the watcher performs its own file appending via `OpenOptions::new().create(true).append(true)` and `file.write_all()` (lines 343-356) without using `write_engine::append_line_fsync`. This misses the fsync guarantee that `write_engine` provides. The `write_engine` crate exists precisely for this use case and is already a workspace dependency of other capability crates.

The watcher does not list `write_engine` in its `Cargo.toml` dependencies at all — it uses only `diff_core` and `datagram_io`.

**Severity:** Medium. The divergence from write_engine means transcript data is not fsync'd, creating a data-loss window on crash.

### OBSERVATION: `convert_json_to_toml` correctly composes

Uses `format_core::convert::strip_nulls`, `format_core::serialize::to_toml`, and `write_engine::write_file_atomic`. This is the correct pattern.

---

## P5: Naming Encodes Architecture

### KNOWN EXCEPTION: `announce` in `cli/` — no verb prefix

`announce` lives in `cli/` but has no verb prefix from the naming table. It is listed in `deploy_categories.toml` under `[tools]`, not under `[senders]`. The CONTEXT_MAP.md acknowledges this as a naming exception. The binary does TTS synthesis and playback — it is not a simple sender, so `send_announce` would be misleading. However, the lack of a verb prefix means an LLM encountering it cannot infer its category from the name alone.

### KNOWN EXCEPTION: `hush` in `cli/` — no verb prefix, is actually a hook

`hush` lives in `cli/` and deploys under `[tools]`, but it is primarily a `UserPromptSubmit` hook that kills announce processes. Its naming gives no indication of its hook nature or its category. An LLM seeing `hush` cannot infer what it does.

### KNOWN EXCEPTION: `traffic_interceptor_rewriter` — should be `intercept_traffic_rewrite`

Documented in CONTEXT_MAP.md. The naming inverts the convention (domain-first instead of verb-first).

### OBSERVATION: `datagram_core` naming is correct

The MEMORY.md mentions `datagram_types` as a naming exception, but the actual crate is `datagram_core` — this has been fixed. The memory entry is stale.

### OBSERVATION: All other naming is correct

All gate, check, writer, hook, sender, converter, rewriter, dispatcher, watcher, interceptor, and daemon crates follow the `{verb}_{domain}` naming convention. All core crates use `_core` suffix. No capability crate uses `_core` suffix. Directory = package = binary name verified across all crates.

---

## P6: Security Hook Coverage

### STRONG: `hook_pre_llm_bash` — excellent dual-direction coverage

53 tests covering: subversion detection (6 positive), truncation detection (6 positive), evasion detection (5 positive), destruction detection (7 positive + 3 negative), revert detection (5 positive + 4 negative), benign commands (5 negative), severity mapping (2), exemption (1), command truncation (4), workflow detection (5 positive + 3 negative). Both directions tested for every category.

### STRONG: `hook_pre_subagent_bash` — excellent dual-direction coverage

36 tests covering: chain detection (10 positive + negative), bare name validation (10), heredoc parsing (7), writer validation (9), inspect path validation (10). Both allow and deny paths tested.

### STRONG: `hook_pre_llm_tool` — solid coverage

25 tests covering all layers (floor, probing, gaming), allow-path exemptions, per-rule severity overrides, and priority ordering. Both block and allow paths tested.

### STRONG: `hook_pre_subagent_tool` — solid coverage

18 tests covering path prefix matching, traversal attacks, unknown tools, missing tools, file_path vs path field precedence.

### VIOLATION: `hook_stop_llm_tts` — ZERO tests

The `hook_stop_llm_tts` binary (123 lines) has no test module at all. It contains pure logic that is testable:
- `StopEvent` deserialization from JSON
- QUIET.lock file checking logic
- The `text_core::strip_markdown` call and empty-check logic
- Project directory resolution priority (CLI > stdin cwd > None)

While this hook does not enforce security policy (it controls TTS playback), the conventions state all tests must pass before committing, and the binary structure guide says every function below main should be testable. The lack of tests means the deserialization contract and the priority logic are unverified.

**Severity:** Medium. Not a security hook, but untested code in a hook binary.

### OBSERVATION: `hook_post_llm_tool` — minimal but present

8 tests covering file classification and path extraction. The actual `assess_source()` function spawns external processes (`saga`, `syn`) so it's inherently an integration test target, which is acceptable.

---

## P7: Schema-First Data Validation

### OBSERVATION: Hook input wire format has no JSON Schema

The hook system reads JSON from stdin and parses it into `HookInput`/`PostHookInput`/`StopEvent` structs using typed Rust deserialization. There is no `.schema.json` file for the hook input format. This is documented as a known issue in CONTEXT_MAP.md.

All other data paths use proper schema validation: gates use `gate_io` with embedded schemas, writers use `write_engine` with `schemas_embedded`, the interceptor uses `cc_wire_schema.json`, and datagrams are validated via `datagram_io::emit_validated`.

### OBSERVATION: syn config has no schema

`.syn/warn.toml` and `.syn/deny.toml` are parsed with a typed `SynFilterToml` struct (syn_cli line 36) but have no schema file. Documented as known issue.

---

## P8: Stale Tests After Contract Changes

### NO VIOLATIONS FOUND

Test assertions across the workspace match current contracts. The `HookDecision` variants (Allow, Deny, Warn, Ask) used in tests match the actual enum definition. Schema field names in test JSON match current schemas. No evidence of phantom contract testing.

---

## P9: Gleipnir Check Accuracy

### OUT OF SCOPE

Per the audit guide, gleipnir accuracy requires running gleipnir against the codebase and evaluating its output for false positives/negatives. This audit reviewed code structure, not gleipnir output. The known improvement areas documented in the audit guide (clone_spam ownership patterns, string_abuse return position, nesting_depth match arms, println main exemption) are pre-existing and tracked.

---

## P10: Orphaned Artifacts

### FINDING: Untracked `senders/announce/` directory

Git status shows `senders/announce/` as an untracked directory. The `announce` binary lives in `cli/announce/` and is deployed under `[tools]`. The `senders/announce/` directory appears to be an abandoned or in-progress relocation attempt. It should be deleted if empty/unused.

### FINDING: `io_filter` crate — confirmed removed

The MEMORY.md mentions `io_filter crate orphaned — imported by nothing, zero tests`. It is not in the workspace `Cargo.toml` members list and no directory was found. This has been cleaned up. The memory entry is stale.

### FINDING: `datagram_types` naming exception — resolved

The MEMORY.md mentions `datagram_types naming exception (should be datagram_core)`. The actual crate is `datagram_core` and lives in `core/datagram_core/`. The rename has been completed. The memory entry is stale.

### OBSERVATION: All workspace members point to real crates

Every entry in workspace `Cargo.toml` members list corresponds to an existing directory with a valid `Cargo.toml`. Every binary crate in the workspace is listed in `deploy_categories.toml`. No orphaned entries.

---

## Summary of Findings

| Priority | Finding | Severity |
|----------|---------|----------|
| P1 | `watch_and_diff_exchange_intercepts` reimplements transcript JSONL append without write_engine/fsync | Medium |
| P1 | `split_jsonl_batches` contains pure `compute_batches()` algorithm extractable to core | Low |
| P4 | `watch_and_diff_exchange_intercepts` does not use write_engine for file writes | Medium |
| P6 | `hook_stop_llm_tts` has zero tests | Medium |
| P3 | `intercept_replay` main() has scattered exit pattern (not broken, but fragile) | Low |
| P1 | `announce` at 460 lines is large but mostly orchestration (borderline) | Low |
| P10 | Untracked `senders/announce/` directory — likely abandoned | Low |
| P10 | Stale MEMORY.md entries for `io_filter` and `datagram_types` (both resolved) | Informational |
| P5 | `announce`, `hush`, `traffic_interceptor_rewriter` naming exceptions (all known, documented) | Known/Deferred |
| P7 | Hook input and syn config lack JSON Schema files (known, documented) | Known/Deferred |

### What Is Correct

- **Three-tier model is clean.** No tier violations. No binary-to-binary deps. No I/O in core crates.
- **process::exit discipline is strong.** All exits in main(), all helpers return Result.
- **Security hooks have excellent test coverage.** Both directions tested, severity mapping verified, exemption paths tested.
- **Naming is consistent** across 95 workspace members (3 documented exceptions).
- **Deploy categories are complete.** Every workspace binary maps to a deploy category.
- **Schema validation is used correctly** for all structured data paths except hooks and syn config (known, documented).
- **Binary sizes are appropriate.** Writers are ~24 lines. Senders are ~25-27 lines. Check CLIs are ~20-27 lines. The architecture is working as designed.
