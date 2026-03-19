# Strict Architectural Audit A

**Date:** 2026-03-19
**Auditor:** A
**Scope:** Full workspace — all 97 workspace members audited against AUDIT_GUIDE.md P1–P10

---

## Summary

The workspace is structurally sound. The three-tier architecture is intact, security hooks have excellent dual-direction test coverage, and the recently extracted `intercept_core` + `session_io` crates demonstrate correct refactoring discipline. The most significant finding is the `announce` binary — a 650-line monolith with no tests, extensive `process::exit()` violations, and substantial pure logic trapped in a binary crate. Several smaller issues exist in other binaries.

**Critical findings:** 1
**Moderate findings:** 6
**Minor findings:** 5

---

## P1: Pure Logic Must Not Live in Binary Crates

### CRITICAL — `senders/announce/src/main.rs` (650 lines, 0 tests)

The `announce` binary is the single worst P1 violation in the workspace. It contains substantial pure logic that should be in core crates:

1. **`compute_hash()`** (lines 402–410) — Pure SHA-256 hash computation over text, voice, speed, model, and language. Deterministic, no I/O.
2. **`sanitize_filename()`** (lines 412–425) — Pure string transformation: lowercase, filter non-alphanumeric, truncate. Deterministic, no I/O.
3. **`build_cache_path()`** (lines 427–432) — Pure path construction (except the `create_dir_all` side effect that should be separated).
4. **`build_tts_body()`** (lines 438–449) — Pure JSON construction for the ElevenLabs API.
5. **`pcm_s16le_to_f32()`** (lines 643–650) — Pure audio format conversion.
6. **Config resolution logic** (`resolve_settings`, lines 293–351) — Four-layer config merge (CLI > VOICE.lock > profile > default). This is a pure priority-merge algorithm mixed with a single I/O call (`load_api_key`).
7. **`apply_lookup()`** (lines 359–372) — Lookup table matching logic mixed with file reads.

This binary has **zero tests**. None of the pure functions listed above are tested. The `sanitize_filename` function, `compute_hash`, and config resolution logic are the type of code that silently breaks when requirements change — and without tests, there is no detection mechanism.

### MODERATE — `daemons/record_datagrams/src/main.rs` `today()` (lines 58–95)

The `today()` function is a 37-line pure date computation (epoch seconds to YYYY-MM-DD civil date). It is deterministic, takes no parameters (uses system time, but the core algorithm is pure given seconds input), and is exactly the kind of utility that another binary will eventually need. Currently untestable because it's entangled with `SystemTime::now()`.

**Recommendation:** Extract to a function `fn civil_date(epoch_secs: u64) -> String` in a core crate, inject the timestamp.

### MODERATE — `dispatchers/split_jsonl_batches/src/main.rs` `compute_batches()` (lines 70–98)

Pure batch-sizing algorithm — takes `(total, min, max)` and returns a `Vec<usize>`. Well-tested within the binary (8 tests), but trapped here. If any future tool needs batch computation, this function is invisible.

### MINOR — `hooks/hook_stop_llm_tts/src/main.rs` `filter_text()` (lines 109–142)

Pure markdown stripping function. Takes text, returns filtered text. Well-tested (8 tests), but if any other binary needs markdown-to-plain-text conversion, this function is invisible. Lower urgency because the use case is narrow.

### MINOR — `hooks/hook_pre_llm_bash/src/main.rs` `make_decision()` (lines 240–292)

Pure function that maps `(Severity, category, description, command) -> HookDecision`. Already well-tested. Shared with `hook_pre_llm_tool/src/main.rs` (which has its own `make_decision` with identical logic). This is a live reimplementation — two binaries contain the same pure function with the same signature and same behavior. One should be in `hook_io`.

---

## P2: The Three-Tier Dependency Model

### No tier violations found.

- No binary crate depends on another binary crate.
- All core crates depend only on other core crates and workspace external deps.
- All capability crates depend on core and other capability crates.
- `schemas_embedded` remains in `capability/` despite having no I/O — known deferred issue, documented in CONTEXT_MAP.md.

### Naming anomalies (known, deferred):

- `core/datagram_types` — should be `datagram_core` per the `_core` suffix convention.
- `interceptors/traffic_interceptor_rewriter` — should be `intercept_traffic_rewrite` per verb-prefix convention.

---

## P3: process::exit and Panic Discipline

### CRITICAL — `senders/announce/src/main.rs`

`process::exit()` appears in **21 locations** across the file, in at least 10 different helper functions:

- `get_input_text()` — 3 exit calls (lines 225, 231, 236)
- `load_api_key()` — 1 exit call (line 272)
- `stream_and_play()` — 2 exit calls (lines 474, 478)
- `download_cache_and_play()` — 2 exit calls (lines 509, 513)
- `play_pcm_from_file()` — 3 exit calls (lines 575, 586, 594)
- `play_audio_file()` — 4 exit calls (lines 611, 619, 627, 635)
- `main()` — 4 exit calls (lines 141, 145, 173, 190) — these are acceptable since they're in main, though 2 are for internal subcommand dispatch which is borderline

Every helper function with `process::exit()` is untestable — calling it from a test kills the test harness. This is the textbook violation described in AUDIT_GUIDE.md P3.

**Recommendation:** Convert all helper functions to return `Result<T, String>`. Keep `process::exit()` only in `main()`.

### All other binaries: Compliant.

Every other binary in the workspace follows the `main()` → `run()` → Result pattern correctly. The hooks use `ExitCode` returns. Writers are 16–24 line declarative shells. Check CLIs are 20–27 line shells.

---

## P4: Composition Over Reimplementation

### MODERATE — `make_decision()` duplicated across hook binaries

`hook_pre_llm_bash` (lines 240–292) and `hook_pre_llm_tool` (lines 163–208) each contain their own `make_decision()` function that maps `Severity` + metadata to `HookDecision`. The implementations are structurally identical — same match arms, same string formatting patterns. This should live in `hook_io` as a shared function.

### MODERATE — `announce` does its own file I/O instead of using `write_engine`

The `announce` binary does raw `fs::write()` for cache files (line 504) and `fs::write()` for hits table (line 285). It should use `write_engine::append_line_fsync` or at minimum pattern-match the atomic write discipline used elsewhere.

### MODERATE — `announce` resolves `$HOME` directly instead of using `write_engine::ai_home()`

`resolve_voice_dir()` (line 244) and `resolve_audio_dir()` (line 249) both read `std::env::var("HOME")` directly. The convention says to use `write_engine::ai_home()` for runtime `$HOME/.ai` resolution.

### MODERATE — `hook_stop_llm_tts` and `hook_post_llm_tool` both resolve `$HOME` directly

`voice_dir()` (line 95–98 in hook_stop_llm_tts) and `tools_bin()` (line 92–95 in hook_post_llm_tool) both read `std::env::var("HOME")` directly instead of using `write_engine::ai_home()`.

---

## P5: Naming Encodes Architecture

### MODERATE — `senders/announce` violates verb prefix convention

Convention: senders use `send_` prefix. The binary is named `announce`, not `send_announce` or `send_tts`. An LLM encountering this crate would not infer it belongs in `senders/` or that it follows the sender pattern. It also does not follow the sender pattern — senders are ~25-line thin wrappers on `datagram_io::emit()`. This binary is a 650-line full application with HTTP client, audio playback, caching, config management, and subprocess spawning.

**Assessment:** `announce` is architecturally misplaced. It is not a sender — it is a standalone tool. It should either be in `cli/` as a specialist tool (like `saga_cli`/`syn_cli`) or decomposed into a core crate (pure logic), a capability crate (API client, audio playback), and a thin binary.

### MINOR — `gate_paths_verified` lacks `_input`/`_output` suffix

Convention: `gate_{stage}_{direction}` where direction is `input` or `output`. `gate_paths_verified` has no direction suffix. It's a passthrough gate (reads, validates, writes) which doesn't fit neatly into the input/output model, but the naming inconsistency means an LLM may not correctly classify it.

### MINOR — `hook_stop_llm_tts` uses non-standard verb

Convention: hooks use `hook_` prefix. This crate uses `hook_stop_` which doesn't map to a standard hook event type. The actual CC hook type is `Stop` (not `PreToolUse` or `PostToolUse`). The name is descriptive but doesn't follow the `hook_pre_`/`hook_post_` pattern used by all other hooks. This is a minor concern since the `Stop` event type is genuinely different from pre/post tool hooks.

---

## P6: Security Hook Coverage

### Strong. Both `hook_pre_llm_bash` and `hook_pre_subagent_bash` have extensive dual-direction test coverage.

**hook_pre_llm_bash:** 70+ tests covering:
- Subversion: 6 positive detections (rm lock, chflags, env override, chmod, flock)
- Truncation: 6 positive detections (head/tail/grep/sed/awk on constraint files)
- Evasion: 5 positive detections (git checkout/restore, mv/cp constraint files, git config hooks)
- Destruction: 7 positive + 3 negative (git reset --hard, checkout ., clean -f vs. soft reset, checkout -b, clean -n)
- Revert: 5 positive + 4 negative (checkout --, restore, stash vs. restore --staged, stash list/pop/show)
- Workflow: 5 positive + 3 negative (cargo build, maturin vs. cargo test/check/clippy)
- Benign commands: 5 explicit non-match tests (ls, cargo build, cat, git status, git diff)
- Severity mapping: 2 tests
- Exemptions: 1 test
- Edge cases: 4 tests (command truncation at 57/60/61 chars)

**hook_pre_subagent_bash:** 50+ tests covering:
- Chain characters: 11 tests (semicolon, &&, ||, backtick, $(), single & ok, empty)
- Bare name validation: 11 tests (valid stems, path separators, hidden files, flags, empty, backslash, dotdot, null byte)
- Heredoc parsing: 8 tests (basic, with name arg, not heredoc, unquoted/double-quoted/lowercase delimiter, no pipe)
- Writer validation: 9 tests (valid heredoc, unknown writer, bad name arg, non-heredoc, echo pipe variants, wrong delimiter, valid name arg, leading dot/dash)
- Inspect validation: 10 tests (path under/outside prefix, chaining, path traversal, command substitution, no paths, relative paths, multiple paths)

**hook_pre_llm_tool:** 20+ tests with floor (always-block), probing, gaming, allow-path exemptions, priority ordering, per-rule severity overrides.

**hook_pre_subagent_tool:** 18 tests covering tool-path mapping, unknown tools, path traversal, file_path vs path precedence.

No gaps identified in security hook test coverage.

---

## P7: Schema-First Data Validation

### Compliant.

- All structured data validated via `schemas_embedded` + `schema_core`.
- 20 agent schemas + 7 tool schemas properly embedded.
- Wire format schema used by `traffic_interceptor_rewriter` and `intercept_replay`.
- No procedural shape-checking found that duplicates schema validation.
- `galdr-style.schema.json` added to `schemas_embedded` (GALDR_STYLE).

### Note: No schema exists for hook input wire format or syn config (.syn/warn.toml, .syn/deny.toml). These are documented as known issues in CONTEXT_MAP.md.

---

## P8: Stale Tests After Contract Changes

### No stale tests identified.

The recent `intercept_core` + `session_io` extraction created new tests that match the current API contracts. The `intercept_replay` binary's `process_line()` function properly uses the new `session_io` API. The `traffic_interceptor_rewriter` was refactored to call `session_io::record_compaction()` for shared compaction flow, and its tests verify the current contract.

---

## P9: Gleipnir Check Accuracy

### Not audited in depth.

Gleipnir's checks are documented in the audit guide as mechanical enforcement via tree-sitter AST analysis. The audit guide explicitly states "Do not duplicate this coverage in architectural audits." No false-positive reports were encountered during this audit.

---

## P10: Orphaned Artifacts

### MINOR — `capability/io_filter` is orphaned

Zero tests, imported by nothing. The crate provides a `run_filter()` function (37 lines) that no binary uses. This has been a known issue since 2026-03-13 (documented in CONTEXT_MAP.md). It should be either integrated into a consumer or removed.

### Workspace alignment: Perfect.

Every workspace member in `Cargo.toml` has a corresponding entry in `deploy_categories.toml`. No phantom deploy entries, no undeploy-able crates.

### Documentation: Current.

CONTEXT_MAP.md lists 85 workspace members but the actual count is now 97 (new crates: `default_apply_core`, `default_apply_io`, `session_io`, `intercept_core`, `intercept_replay`, `announce`, `hook_stop_llm_tts`, `gate_raw_definition_defaults`, `gate_galdr_style_input`, and others added since the last context map refresh). The CONTEXT_MAP.md crate inventory is stale.

---

## Findings Summary Table

| ID | Priority | Category | Location | Finding |
|----|----------|----------|----------|---------|
| A1 | CRITICAL | P1+P3 | `senders/announce` | 650-line monolith, 0 tests, 21 process::exit calls in helpers, substantial pure logic trapped in binary |
| A2 | MODERATE | P1 | `daemons/record_datagrams` | `today()` pure date computation trapped in binary |
| A3 | MODERATE | P1 | `dispatchers/split_jsonl_batches` | `compute_batches()` pure algorithm trapped in binary |
| A4 | MODERATE | P4 | `hook_pre_llm_bash` + `hook_pre_llm_tool` | `make_decision()` duplicated across two hooks — should be in `hook_io` |
| A5 | MODERATE | P4 | `senders/announce` | Direct `fs::write()` and `std::env::var("HOME")` instead of `write_engine` and `ai_home()` |
| A6 | MODERATE | P4 | `hook_stop_llm_tts` + `hook_post_llm_tool` | Direct `std::env::var("HOME")` instead of `write_engine::ai_home()` |
| A7 | MODERATE | P5 | `senders/announce` | Missing `send_` prefix, architecturally misplaced — not a sender, is a full application |
| A8 | MINOR | P1 | `hooks/hook_stop_llm_tts` | `filter_text()` pure markdown stripping trapped in binary |
| A9 | MINOR | P5 | `gates/gate_paths_verified` | Missing `_input`/`_output` suffix |
| A10 | MINOR | P5 | `hooks/hook_stop_llm_tts` | Non-standard `hook_stop_` verb pattern |
| A11 | MINOR | P10 | `capability/io_filter` | Orphaned crate — zero tests, zero consumers |
| A12 | MINOR | P10 | `CONTEXT_MAP.md` | Stale crate inventory (lists 85 members, actual is 97) |
