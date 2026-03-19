# Nornir Improvement Plan

Generated 2026-03-19 from intersection of strict_audit_A.md and strict_audit_B.md.

---

## 1. Decompose `announce` monolith

**Priority: Critical** (P1+P3+P4+P5+P7+P8 — 6 simultaneous violations)

650 lines, 0 tests, 21 `process::exit()` in helpers, no `write_engine`/`ai_home()`, no schema validation, misplaced in `senders/`.

- Extract pure logic to `core/announce_core`: `compute_hash`, `sanitize_filename`, `build_cache_path`, `build_tts_body`, `pcm_s16le_to_f32`, config resolution
- Convert all helpers to return `Result<T, String>`, keep `process::exit()` only in `main()`
- Use `write_engine::ai_home()` for path resolution
- Add unit tests for all extracted pure functions
- Move binary from `senders/` to `cli/` (it's a full TTS application, not a sender)
- Update deploy_categories.toml

## 2. Fix `$HOME` hardcoding in hooks

**Priority: Moderate** (P4 — composition violation)

`hook_post_llm_tool` and `hook_stop_llm_tts` both manually read `$HOME` and construct `~/.ai/tools/bin`. Should use `write_engine::ai_home()`.

- Add `write_engine` dep to both hook crates
- Replace `std::env::var("HOME")` with `write_engine::ai_home()`
- Verify tests still pass

## 3. Extract `make_decision()` to `hook_io`

**Priority: Moderate** (P4 — duplicated pure logic)

`hook_pre_llm_bash` and `hook_pre_llm_tool` each contain their own `make_decision()` with identical logic mapping `Severity` + metadata to `HookDecision`.

- Move shared `make_decision()` to `hook_io`
- Both hooks call the shared function
- Existing tests stay in the hook binaries (they test integration)

## 4. Extract `record_datagrams` `today()` to a core utility

**Priority: Moderate** (P1+P4 — pure logic in binary, reimplementation risk)

37-line hand-rolled epoch-to-civil-date converter. Untestable as-is (uses `SystemTime::now()` directly).

- Extract to `fn civil_date(epoch_secs: u64) -> String` in an appropriate core crate
- Inject timestamp in caller
- Add unit tests for edge cases (midnight, leap year, etc.)

## 5. Refresh CONTEXT_MAP.md

**Priority: Moderate** (P10 — stale inventory)

Lists 85 members, actual is 97. Missing: `intercept_core`, `session_io`, `intercept_replay`, `announce`, `hook_stop_llm_tts`, `default_apply_core`, `default_apply_io`, `gate_raw_definition_defaults`, `gate_galdr_style_input`, and others.

- Regenerate crate inventory from workspace Cargo.toml
- Update test counts
- Update known issues section

## 6. Extract `filter_text()` from `hook_stop_llm_tts`

**Priority: Minor** (P1 — pure logic trapped in binary)

33-line pure markdown stripping function. Reusable if `announce` or other tools need text preprocessing.

- Move to a core crate (could go in `format_core` or a new `text_core`)
- Keep tests alongside the function
- Hook binary calls the shared function

## 7. Delete orphaned `io_filter` crate

**Priority: Minor** (P10 — zero tests, zero consumers)

37-line crate imported by nothing. Known issue since 2026-03-13.

- Remove from workspace Cargo.toml
- Delete `capability/io_filter/` directory
- Remove from deploy_categories.toml if present

## 8. Rename `datagram_types` to `datagram_core`

**Priority: Minor** (P5 — naming exception)

Core crate uses `_types` suffix instead of `_core`. Known deferred issue.

- Rename directory `core/datagram_types` to `core/datagram_core`
- Update package name in Cargo.toml
- Update all dependents (datagram_io, send_datagram, record_datagrams, session_io, etc.)
- Update workspace Cargo.toml

## 9. Extract `watch_and_diff` pure functions

**Priority: Minor** (P1 — accumulation pattern)

`jitter_sleep` (xorshift64 PRNG), `parse_pace` (string parsing), `workspace_from_parent_dir` (path manipulation), `accumulate_line` (JSON parsing) are pure logic in a binary.

- Small individually but represents accumulation
- Extract to appropriate core/capability crates when the binary next needs changes

## 10. Add schema validation for `announce` TOML configs

**Priority: Minor** (P7 — no schema validation)

`announce.toml`, `VOICE.lock`, lookup tables, `hits.toml` are parsed via serde with no schema files. Addressed partially by item #1 decomposition.

---

**Process:** Work items top-down. For each: plan mode → execute → verify → commit → next.
