# Strict Architectural Audit B

**Auditor:** B
**Date:** 2026-03-19
**Scope:** Full workspace audit against AUDIT_GUIDE.md invariants P1-P10

---

## P1: Pure Logic in Binary Crates

### VIOLATION: `announce` (senders/announce/) — 650-line monolith with extensive pure logic

The `announce` binary at 650 lines is the largest single violation in the workspace. It contains substantial pure logic that should be in a core or capability crate:

1. **`compute_hash()`** (line 402) — pure SHA-256 hash computation over text+voice+speed+model+lang. Deterministic, no I/O.
2. **`sanitize_filename()`** (line 412) — pure text-to-filename conversion. Deterministic string transformation.
3. **`build_cache_path()`** (line 427) — partially pure path construction (calls `create_dir_all` inline, but the path logic itself is pure).
4. **`build_tts_body()`** (line 438) — pure JSON body construction for the ElevenLabs API.
5. **`pcm_s16le_to_f32()`** (line 643) — pure PCM audio conversion. Takes bytes, returns floats.
6. **Config resolution logic** (`resolve_settings`, lines 293-351) — multi-layer config merging (CLI > VOICE.lock > profile > default) is pure priority-merge logic once the inputs are resolved.
7. **`apply_lookup()`** (line 359) — lookup table resolution mixes I/O (file read) with pure matching logic that could be separated.
8. **`filter_text()`** in `hook_stop_llm_tts` (line 109) — pure markdown stripping logic. If announce grows a text preprocessing pipeline, this will be reimplemented.

This binary is on a trajectory to become the monolith the audit guide warns about. An `announce_core` crate should hold hash computation, filename sanitization, config merging, and audio conversion.

### VIOLATION: `record_datagrams` — hand-rolled date formatting

The `today()` function (line 58, 37 lines) is a hand-rolled epoch-to-civil-date converter. This is pure deterministic math that belongs in a shared utility, not reimplemented in a daemon binary. If any other binary ever needs date-stamped filenames, this will be copied.

### VIOLATION: `watch_and_diff_exchange_intercepts` — `jitter_sleep` and `parse_pace`

- `jitter_sleep()` (line 74) implements xorshift64 PRNG — pure math.
- `parse_pace()` (line 59) is pure string parsing that returns Result.
- `workspace_from_parent_dir()` (line 86) is pure path manipulation.
- `accumulate_line()` (line 216) is pure line accumulation/JSON parsing logic.

These are small individually but represent the accumulation pattern the audit guide describes.

### VIOLATION: `hook_stop_llm_tts` — `filter_text()` pure logic in binary

The `filter_text()` function (line 109, 33 lines) is entirely pure: takes a string, returns a string. It strips markdown fenced code blocks, tables, inline code, bold, italic, headers, and collapses blank lines. This is reusable text processing that should not be trapped in a hook binary. If `announce` ever wants to pre-filter text, this logic will be reimplemented.

### OBSERVATION: `split_jsonl_batches` — `compute_batches()` is pure and well-isolated

`compute_batches()` (line 70) is pure batch-size computation. It's small (28 lines) and well-tested, but it is pure logic in a binary. Minor — could reasonably stay unless another binary needs batch computation.

### CLEAN: Writers, senders (excluding announce), check CLIs, hooks (excluding hook_stop_llm_tts)

All writer binaries are 16-24 lines of declarative config — exemplary. Simple senders (send_heartbeat, send_alert, send_warning, send_notification) are 25-27 lines. Check CLIs are 20-27 lines. The hook binaries (hook_pre_llm_bash, hook_pre_llm_tool, hook_pre_subagent_bash, hook_pre_subagent_tool) correctly delegate pure logic to `hook_io::rules` and keep only decision orchestration. `rewrite_compaction_summary` correctly delegates to `compaction_inject_core`. `saga_cli` and `syn_cli` correctly delegate to `saga_runner`, `syn_core`, and `report_render_core`.

---

## P2: Three-Tier Dependency Model

### VIOLATION: `announce` binary depends on no core or capability crates

`announce` has zero internal dependencies — it imports only external workspace crates (clap, serde, toml, sha2, reqwest, dotenvy, rodio). This means every piece of infrastructure it needs (config file reading, path resolution, hashing, file I/O) is reimplemented locally. It does not use `write_engine::ai_home()` for path resolution — it constructs `$HOME/.ai/voice` manually (line 244-246). It does not use `write_engine` for any file writes.

### OBSERVATION: `schemas_embedded` in capability/ but has no I/O

As noted in CONTEXT_MAP.md known issues. `schemas_embedded` only calls `include_str!()` (compile-time) and creates `EmbeddedValidator` instances. It has no runtime I/O. It is misplaced in capability/ — it belongs in core/ or as a special tier-1 crate. Deferred.

### OBSERVATION: `datagram_types` naming exception

In core/ but named `datagram_types` instead of `datagram_core`. Already documented in CONTEXT_MAP.md as deferred.

### CLEAN: No binary-to-binary dependencies detected

Checked all Cargo.toml files. No binary crate depends on another binary crate. The `intercept_replay` binary depends on `intercept_core` (core) and `session_io` (capability) — correct. The `traffic_interceptor_rewriter` depends on `intercept_core`, `session_io`, `compaction_inject_core`, and `schema_core` — correct tier boundaries.

### CLEAN: Core crates have no I/O

All 13 core crates checked: none import `std::fs`, `std::net`, or `std::env`. `gleipnir_core` uses tree-sitter (which parses in-memory buffers, not files) — correct. `default_apply_core` is pure JSON manipulation — correct.

---

## P3: process::exit and Panic Discipline

### VIOLATION: `announce` — process::exit() in 15+ helper functions

`announce` has `process::exit()` scattered across helper functions:
- `get_input_text()` (lines 225, 231, 236) — exits on empty input
- `load_api_key()` (line 272) — exits when API key missing
- `stream_and_play()` (lines 474, 478) — exits on API error
- `download_cache_and_play()` (lines 509, 513) — exits on API error
- `play_pcm_from_file()` (lines 575, 586, 594) — exits on decode/output errors
- `play_audio_file()` (lines 611, 619, 627, 635) — exits on decode/output errors

This makes the entire binary untestable. Every function that calls `process::exit()` kills the test harness on error paths. The binary has zero tests.

### OBSERVATION: `announce` main() also has early exits (lines 141, 145, 151, 173, 190)

These are in `main()` itself, which is acceptable per convention. However, the proliferation masks the helper-function exits.

### CLEAN: All other binaries

All other binaries follow the convention correctly:
- Hook binaries return `ExitCode` via `hook_io::run_hook(decide)`.
- Complex binaries (syn_cli, saga_cli, split_jsonl_batches, record_datagrams, watch_and_diff, rewrite_compaction_summary, intercept_replay) use the `fn run() -> Result<T, String>` pattern with `process::exit()` only in `main()`.
- `.unwrap()` calls in production code are limited to `LazyLock` initializers in `hook_pre_subagent_bash` (lines 22, 26, 30) — correctly exempted.

---

## P4: Composition Over Reimplementation

### VIOLATION: `announce` does not use `write_engine::ai_home()` for path resolution

`resolve_voice_dir()` (line 244) and `resolve_audio_dir()` (line 249) manually read `$HOME` and construct `.ai/voice` and `.ai/audio` paths. `write_engine::ai_home()` exists precisely for this purpose and handles the `HOME` env var consistently across the workspace.

### VIOLATION: `hook_post_llm_tool` and `hook_stop_llm_tts` manually construct `~/.ai/tools/bin` path

`tools_bin()` in `hook_post_llm_tool` (line 92) and `announce_bin()` in `hook_stop_llm_tts` (line 101) both read `$HOME` and construct `PathBuf::from(home).join(".ai/tools/bin")`. This is a hardcoded path pattern that should use `write_engine::ai_home()`.

### VIOLATION: `record_datagrams` — hand-rolled date formatting instead of using a shared utility

The `today()` function reimplements epoch-to-civil-date conversion. While no shared date utility currently exists in the workspace, this is a signal that one should be created if date formatting is needed elsewhere.

### CLEAN: Writer, sender, hook, check binaries all compose correctly

Writers use `write_engine::run()`. Senders use `datagram_io::emit()`. Hooks use `hook_io::run_hook()`. Check CLIs use `io_check::run_check()`. Rewriters use core crates (`compaction_inject_core`). The interceptor uses `intercept_core` + `session_io`. This is exemplary composition.

---

## P5: Naming Encodes Architecture

### VIOLATION: `announce` — no verb prefix, wrong category

The binary `announce` lives in `senders/` but has no `send_` prefix. Per the naming convention table, senders use the `send_` prefix. `announce` should be `send_announce` or, given its complexity (650 lines, TTS API integration, audio playback, config management), it arguably belongs in a different category entirely — perhaps `daemons/` or a new `audio/` category.

Additionally, `announce` is not a "fire-and-forget datagram to hlidskjalf socket" — which is the definition of a sender. It makes HTTP API calls, manages an audio cache, spawns child processes for playback, and reads TOML config files. It is architecturally a full application masquerading as a sender.

### VIOLATION: `hook_stop_llm_tts` — verb prefix `hook_stop_` is not in the convention table

The convention specifies `hook_` prefix for hooks. `hook_stop_llm_tts` uses `hook_stop_` which could be read as a different verb prefix. However, this is the CC Stop event hook naming pattern (matching `hook_pre_` and `hook_post_`), so `hook_stop_` is a reasonable extension. Minor.

### OBSERVATION: `intercept_replay` naming

Lives in `interceptors/` and uses `intercept_` prefix — correct per conventions. The name `intercept_replay` accurately describes its function.

### CLEAN: All other crate names match conventions

Verified: All writer names use `append_` or `write_`. All check CLIs use `check_`. All senders (except `announce`) use `send_`. All hooks use `hook_`. All core crates use `_core` suffix (with documented exceptions for `datagram_types`). All capability crates lack `_core` suffix. Directory name = package name = binary name verified for all crates.

---

## P6: Security Hook Coverage

### CLEAN: hook_pre_llm_bash — comprehensive dual-direction test coverage

92 tests covering:
- Subversion: 6 positive detections (rm lock, chflags, export HOOK env, env override, chmod rules.toml, flock unlock) + benign commands verified not flagged
- Truncation: 6 positive detections (head/tail/grep/sed/awk on CLAUDE.md, guardrails piped) + benign cat not flagged
- Evasion: 5 positive detections (git checkout/restore/mv/cp CLAUDE.md, git config hooks) + benign git status/diff not flagged
- Destruction: 7 positive detections (git reset --hard, checkout ., checkout -- ., restore ., clean -f, clean -fd) + 3 benign negatives (reset --soft, checkout -b, clean -n)
- Revert: 5 positive detections (checkout -- file, checkout HEAD -- file, restore file, stash, stash push) + 4 benign negatives (restore --staged, stash list/pop/show, checkout branch)
- Workflow: 5 positive detections (cargo build --release, reordered, cargo install, debug build, maturin) + 3 benign negatives (test, check, clippy)
- Severity overrides verified for debug build (warn) and release build (ask)

### CLEAN: hook_pre_subagent_bash — comprehensive dual-direction test coverage

53 tests covering shell chaining (11 tests), bare name validation (10 tests), heredoc header parsing (7 tests), writer validation (10 tests), inspect path validation (12 tests), integration (3 tests).

### CLEAN: hook_pre_llm_tool — comprehensive dual-direction test coverage

37 tests covering floor rules (always-deny for SSH/AWS/GPG/Kube/Docker/netrc + override resistance), probing (block/warn/disabled), gaming, benign paths, allow_path exemption, priority ordering.

### CLEAN: hook_pre_subagent_tool — comprehensive dual-direction test coverage

19 tests covering path prefix validation (allow/deny), unknown tool denial, no-tool/no-path handling, path traversal, file_path vs path field priority.

---

## P7: Schema-First Data Validation

### VIOLATION: `announce` — no schema validation for any data

The `announce` binary reads TOML config files (`announce.toml`, `VOICE.lock`, lookup tables, `hits.toml`) and parses them into Rust structs via `serde::Deserialize`. There are no JSON Schema files for any of these data shapes. All validation is structural/procedural through Rust's type system. If the config format changes, there is no schema to update — only Rust structs.

### OBSERVATION: Hook input wire format has no JSON Schema

Already documented in CONTEXT_MAP.md known issues. `hook_io` uses typed accessors but no `.schema.json` file defines the hook input format.

### OBSERVATION: Syn config files (.syn/warn.toml, .syn/deny.toml) have no schema

Already documented in CONTEXT_MAP.md known issues.

### CLEAN: All gate pipelines use schema-first validation

All check CLIs validate via `schemas_embedded` constants. All gate modules validate via `gate_io`. The interceptor validates via `WIRE_SCHEMA` (embedded `cc_wire_schema.json`). Writers validate via `write_engine::run()` which checks against embedded schemas. Datagrams are validated via `datagram_io::emit_validated()` against `validate.datagram.schema.json`.

---

## P8: Stale Tests After Contract Changes

### VIOLATION: `announce` has zero tests

The 650-line binary has no `#[cfg(test)]` module at all. This is not a stale-test problem — it is a no-test problem. The pure logic functions (`compute_hash`, `sanitize_filename`, `build_tts_body`, `pcm_s16le_to_f32`, `resolve_settings`, `filter_text`) are all untested. The `process::exit()` calls in helpers make them structurally untestable without extraction.

### CLEAN: All other binaries have meaningful test coverage

- `hook_pre_llm_bash`: 92 tests
- `hook_pre_subagent_bash`: 53 tests
- `hook_pre_llm_tool`: 37 tests
- `hook_pre_subagent_tool`: 19 tests
- `hook_post_llm_tool`: 9 tests
- `hook_stop_llm_tts`: 9 tests
- `watch_and_diff_exchange_intercepts`: 16 tests
- `split_jsonl_batches`: 14 tests
- `record_datagrams`: 7 tests
- `rewrite_compaction_summary`: 11 tests
- `send_datagram`: 12 tests
- `syn_cli`: 4 tests
- Core crates have substantial test suites (gleipnir_core: 267, format_core: 78, hook_io: 40, syn_core: 39, etc.)

---

## P9: Gleipnir Check Accuracy

Not audited in detail — gleipnir_core has 267 tests which suggests robust self-verification. The audit guide notes this is covered mechanically.

---

## P10: Orphaned Artifacts

### VIOLATION: CONTEXT_MAP.md is stale — does not reflect current workspace

The CONTEXT_MAP.md (dated 2026-03-13) lists 85 workspace members but the current workspace Cargo.toml has 97 members. Missing from CONTEXT_MAP:
- `core/default_apply_core` — new core crate
- `core/intercept_core` — new core crate
- `capability/default_apply_io` — new capability crate
- `capability/session_io` — new capability crate
- `interceptors/intercept_replay` — new binary
- `senders/announce` — new binary
- `hooks/hook_stop_llm_tts` — new binary
- Several new gate crates (`gate_raw_definition_defaults`, `gate_galdr_style_input`)

The inventory counts (11 core, 10 capability, etc.) are outdated. The test count (877) is outdated.

### VIOLATION: `io_filter` crate is orphaned

Confirmed: `io_filter` has zero tests, is imported by nothing (checked all Cargo.toml files — no crate lists `io_filter` as a dependency). Already documented in CONTEXT_MAP.md as a known issue but remains unresolved.

### OBSERVATION: deploy_categories.toml may be missing new crates

`default_apply_io` and `default_apply_core` are in the workspace but do not appear in `deploy_categories.toml`. Since they are library crates (not binaries), they don't need deploy entries — they're built as dependencies of gate crates. This is correct behavior but worth noting.

---

## Summary of Findings

### Critical (architectural damage if not addressed)

| # | Finding | Location | Priority |
|---|---------|----------|----------|
| 1 | `announce` is a 650-line monolith with pure logic, zero tests, process::exit in helpers, no composition with workspace crates, no schema validation, wrong naming | `senders/announce/src/main.rs` | P1+P3+P4+P5+P7+P8 |

### Significant (will cause drift if not addressed)

| # | Finding | Location | Priority |
|---|---------|----------|----------|
| 2 | `hook_post_llm_tool` and `hook_stop_llm_tts` hardcode `~/.ai/tools/bin` path instead of using `write_engine::ai_home()` | hooks/ | P4 |
| 3 | CONTEXT_MAP.md is stale — 12+ crates missing from inventory | `CONTEXT_MAP.md` | P10 |
| 4 | `hook_stop_llm_tts` contains pure `filter_text()` in a binary crate | `hooks/hook_stop_llm_tts/src/main.rs` | P1 |

### Minor (documented or low-risk)

| # | Finding | Location | Priority |
|---|---------|----------|----------|
| 5 | `record_datagrams` hand-rolled date formatter | `daemons/record_datagrams/src/main.rs` | P1/P4 |
| 6 | `io_filter` crate orphaned (zero tests, zero imports) | `capability/io_filter/` | P10 |
| 7 | `schemas_embedded` in capability/ but has no I/O | `capability/schemas_embedded/` | P2 |
| 8 | `datagram_types` naming exception (should be `datagram_core`) | `core/datagram_types/` | P5 |
| 9 | Hook input wire format has no JSON Schema | `capability/hook_io/` | P7 |

### Architectural Health Assessment

The workspace is in strong structural health outside of the `announce` binary. The three-tier model is respected. Composition patterns are followed consistently — writers, senders, hooks, and check CLIs are exemplary thin binaries. Security hooks have comprehensive dual-direction test coverage. Schema-first validation is practiced across the pipeline.

The `announce` binary is the single largest source of architectural concern. It violates 6 of 10 audit priorities simultaneously and, at 650 lines with zero tests, represents exactly the monolith accumulation pattern the audit guide warns about. It needs extraction of pure logic to a core crate, restructuring to return `Result` instead of calling `process::exit()`, and comprehensive test coverage.
