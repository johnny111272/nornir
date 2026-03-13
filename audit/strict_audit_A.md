# Strict Audit Report A

Auditor: Claude Opus 4.6
Date: 2026-03-12
Scope: Full workspace audit against AUDIT_GUIDE.md priorities P1--P10

---

## P1: Pure Logic in Binary Crates

**Rule**: Binary crates (main.rs) must be thin orchestration. Pure logic belongs in core/ or capability/ crates. If a helper function has no I/O, it should not live in a binary crate.

### Findings

**P1-1** `cli/syn_cli/src/main.rs` (1197 lines)
- Lines 37--53: `compile_filter()` and `apply_filter()` -- jaq filter compilation and evaluation. These are pure computation functions (no I/O) embedded in a binary crate. Should live in a core crate (e.g., a `filter_core` or be part of `report_render_core`).
- Lines 66--90: `load_filter_config()` mixes config I/O with filter compilation. The compilation portion is pure and should be separated.
- Lines 108--180: `PolicyConfig`, `OutputArgs`, full TOML config parsing and filter management -- structural logic that is not I/O. All pure and testable without being in main.rs.
- Lines 282--350: `run_report()` and `run_gate()` contain decision logic (severity thresholds, exit code computation) that is testable pure logic.
- Lines 390--470: Formatting dispatch, stdin reading, file discovery -- mixed I/O and pure logic in a single binary. The binary is 1197 lines total, making it the largest binary in the workspace.

**P1-2** `interceptors/traffic_interceptor_rewriter/src/main.rs` (618 lines)
- Lines 60--116: `classify_exchange()`, `ExchangeKind` enum, exchange classification logic. This is pure computation on JSON values with no I/O. Should be in a core crate (possibly `diff_core` which already handles exchange structures).
- Lines 210--280: Orchestration logic (`process_exchange()`, routing based on ExchangeKind) contains pure decision logic interleaved with I/O calls.

**P1-3** `watchers/watch_and_diff_exchange_intercepts/src/main.rs` (554 lines)
- Lines 59--69: `parse_pace()` -- pure parsing logic.
- Lines 74--81: `jitter_sleep()` -- contains a pure PRNG (xorshift64) mixed with I/O (`thread::sleep`). The PRNG portion is pure.
- Lines 85--91: `workspace_from_parent_dir()` -- pure path manipulation. This is a candidate for `path_core`.
- Lines 99--155: `diff_and_emit()` -- mixes pure diffing decisions with datagram emission I/O.
- Lines 217--236: `accumulate_line()` -- pure line accumulation logic in a binary crate.

**P1-4** `hooks/hook_pre_llm_bash/src/main.rs` (758 lines)
- Lines 26--74: `Rules` struct, `parse_rules()`, config parsing -- pure TOML parsing logic in a binary.
- Lines 76--180: `decide()` function and all its helpers (`is_allowed_pattern`, `make_decision`, `classify_command`) -- pure decision logic. None of this touches I/O. It should be in a capability or core crate.

**P1-5** `dispatchers/split_jsonl_batches/src/main.rs` (388 lines)
- Lines 88--128: `split_to_batches()` -- pure splitting logic (partitioning lines into groups of N). No I/O involved in the partitioning itself.

**P1-6** `daemons/record_datagrams/src/main.rs` (379 lines)
- Lines 48--87: `today()`, `today_dir()`, JSONL log writing logic that could be extracted. The date-formatting function `today()` is pure.
- Lines 89--116: `log_datagram()` mixes pure file-path construction with I/O operations.

---

## P2: Three-Tier Dependency Model

**Rule**: Core crates (Tier 1) must be pure computation -- no I/O, no env, no fs. Capability crates (Tier 2) wrap I/O around core. Binaries (Tier 3) compose from both tiers.

### Findings

**P2-1** `core/gleipnir_core/src/config.rs` lines 18--19
- `std::fs::read_to_string()` and `candidate.exists()` -- direct filesystem I/O in a core crate.
- This file reads `.gleipnir/user.toml` from disk. This is a Tier 2 operation living in Tier 1.
- The `gleipnir_core` lib.rs docstring claims "No I/O -- caller provides source bytes" but config.rs contradicts this.
- Violates: Core crates must not perform I/O.

**P2-2** `core/format_core/src/tomlx/paths.rs` lines 5, 88, 106, 120
- `std::env::var()` calls (HOME, USERPROFILE, arbitrary env vars via `$VAR` expansion).
- Environment variable access is I/O. A core crate should receive resolved paths from the caller, not probe the environment itself.
- Violates: Core crates must be pure computation.

**P2-3** `core/format_core/src/tomlx/processor.rs` line 410
- `use std::env` in test code -- this is acceptable (test-only), but the production code in paths.rs (P2-2) is not.

**P2-4** `core/diff_core/Cargo.toml` depends on `datagram_types` (core -> core)
- This is valid. Core-to-core dependencies are allowed.
- No violation.

**P2-5** No capability crate depends on another capability crate except `hook_io` -> `datagram` and `gate_io` -> `path_verify` and `schemas_embedded` -> `schema_core`.
- `hook_io` -> `datagram`: Capability -> Capability. This is allowed per the architecture (Tier 2 can depend on Tier 2).
- No violation.

---

## P3: process::exit() and Panic Discipline

**Rule**: Only `main()` may call `process::exit()`. Helpers must return `Result`. No `.unwrap()` or `.expect()` in production (non-test) code, except in static/const initialization contexts.

### Findings

**P3-1** `daemons/record_datagrams/src/main.rs` line 175
- `process::exit(0)` inside `shutdown_handler()` -- an extern "C" signal handler function. This is NOT `main()`. While signal handlers have limited options (cannot return `Result`), this violates the rule as written. The handler is `extern "C" fn shutdown_handler()`, not `fn main()`.

**P3-2** `daemons/record_datagrams/src/main.rs` lines 144, 160, 169, 179, 247
- Five `unsafe` blocks: mutable static access (`SHUTDOWN_SOCKET_PATH`, `SHUTDOWN_FLAG`), raw signal registration. The `unsafe` usage is a signal-handling concern, but the mutable statics pattern is fragile and data-race-prone. This is not a `process::exit` violation per se, but the `unsafe` code is production code that panics-by-UB on concurrent access.

**P3-3** `core/schema_core/src/lib.rs` lines 51--53
- `.expect("embedded schema must be valid JSON")` and `.expect("embedded schema must be valid JSON Schema")` inside `OnceLock::get_or_init()`. These are initialization-context expects, which the gleipnir `no_unwrap` check correctly exempts. Per the audit guide, this is acceptable for compile-time-embedded data that is guaranteed valid. No violation.

**P3-4** `core/gleipnir_core/src/lib.rs` line 26
- `.expect("gleipnir_messages.toml parse error")` inside `LazyLock::new()`. Same as P3-3 -- static initialization of compiled-in data. No violation.

**P3-5** All other `process::exit()` calls are inside `fn main()` blocks:
- `watchers/.../main.rs` lines 382, 399 -- in `fn main()`
- `cli/syn_cli/src/main.rs` lines 482, 487, 490 -- in `fn main()`
- `rewriters/.../main.rs` line 113 -- in `fn main()`
- `converters/.../main.rs` line 57 -- in `fn main()`
- All senders -- in `fn main()`
- `cli/saga_cli/src/main.rs` lines 192, 202, 205 -- in `fn main()`
- `dispatchers/.../main.rs` lines 196, 200, 203 -- in `fn main()`
- `interceptors/.../main.rs` line 287 -- in `fn main()`
- All writers -- in `fn main()`
- No violations for these.

**P3-6** `capability/io_check/src/lib.rs` line 112
- `.unwrap_or_default()` -- this is a safe unwrap variant. No violation.

**P3-7** `capability/saga_runner/src/lib.rs` lines 117, 123, 167, 175, 177
- Multiple `.unwrap_or()` and `.unwrap_or_default()` calls on JSON field access. These are safe unwrap variants. No violation.

---

## P4: Composition Over Reimplementation

**Rule**: Compose from existing crates. Do not reimplement what a crate already provides. If `write_engine::write_file_atomic` exists, use it instead of raw `std::fs::write`.

### Findings

**P4-1** `rewriters/rewrite_compaction_summary/src/main.rs` line 66
- `std::fs::write(&path, content)` for debug snapshot output. `write_engine::write_file_atomic` exists for safe file writes with rename. However, this is debug-only output (gated behind `--debug`), so the argument for atomic writes is weaker. Still, the rule says to compose from existing crates.

**P4-2** `interceptors/traffic_interceptor_rewriter/src/main.rs` lines 127, 155, 184
- Three separate `OpenOptions::new().create(true).append(true).open()` calls with manual `write_all`/`flush`/`sync_all` patterns. These are append-mode JSONL operations (not overwrite), so `write_file_atomic` does not directly apply. However, the append pattern is duplicated three times within the same file. This is internal duplication, not cross-crate reimplementation.

**P4-3** `watchers/watch_and_diff_exchange_intercepts/src/main.rs` lines 350--368
- `open_transcript()` and `append_transcript()` implement their own file append logic with `OpenOptions`. Same pattern as P4-2 -- append-mode JSONL. Internal to the binary but duplicates the interceptor pattern.

**P4-4** `daemons/record_datagrams/src/main.rs` lines 89--116
- `log_datagram()` implements yet another JSONL-append pattern. Same `OpenOptions` + `write_all` + `flush`. This is the fourth occurrence of the append-JSONL pattern across binaries. A shared `append_jsonl()` helper in a capability crate would eliminate this duplication.

**P4-5** `interceptors/traffic_interceptor_rewriter/src/main.rs` lines 60--116
- `classify_exchange()` reimplements exchange-type classification. `diff_core` already has `split_exchange()` and related functions. The interceptor does not use `diff_core` for classification, instead implementing its own `ExchangeKind` enum and `classify_exchange()` function that examines the same JSON structure.

---

## P5: Naming Encodes Architecture

**Rule**: Directory name = package name = binary name (with documented exceptions for saga_cli->saga, syn_cli->syn). Verb-prefix naming required. Tier suffixes: `_core` for core, `_io`/`_engine`/`_runner` etc. for capability.

### Findings

**P5-1** `capability/datagram` -- crate name `datagram`
- No tier-indicating suffix. Capability crates should indicate their nature. This crate performs I/O (Unix socket, UDP multicast) but its name `datagram` gives no indication it is a capability crate. It should be something like `datagram_io` or `datagram_transport` to indicate it is Tier 2.
- The pure types are correctly separated in `core/datagram_types`. But `datagram` alone does not follow the naming pattern of other capability crates: `hook_io`, `gate_io`, `io_check`, `io_filter`, `saga_runner`, `write_engine`, `path_verify`, `intercept_io`, `schemas_embedded`.

**P5-2** `capability/schemas_embedded` -- name `schemas_embedded`
- No verb prefix. Other capability crates use verb-noun or noun-verb patterns with role suffixes. `schemas_embedded` is a description, not an action. However, this is a data crate (static schema storage), not an action crate, so this may be an acceptable exception. The naming guide does not explicitly address data-only crates.

**P5-3** `writers/write_truth_glossary_record` and `writers/append_truth_qc_report_record`
- The naming convention says writers use `append_` or `write_` prefix. These follow it. No violation.

**P5-4** `interceptors/traffic_interceptor_rewriter`
- Documented exception in NORNIR_NAMING.md. No violation.

**P5-5** `capability/path_verify`
- No `_io` or similar suffix. Its function is to verify paths (I/O operation -- stat/exists checks). The name `path_verify` does not indicate tier. Compare with `io_check`, `io_filter`. Should be `path_verify_io` or similar, though it is closer to a verb-noun pattern.

---

## P6: Security Hook Coverage

**Rule**: Security hooks must have dual-direction test coverage: tests that confirm blocking (deny/warn), AND tests that confirm allowing (benign cases pass through).

### Findings

**P6-1** `hooks/hook_pre_llm_tool/src/main.rs`
- Allow-direction tests: `decide_benign_path_not_flagged` (line 410), `decide_no_path_in_input_allows` (line 421), `is_allowed_path_*` (multiple tests).
- Deny-direction tests: `decide_floor_ssh_always_denied` (line 332), floor tests for AWS/GPG/Kube/Docker/Netrc, probing tests for hooks/gleipnir/settings.
- Coverage: Both directions covered. However, the deny-direction tests do NOT call `decide()` directly -- they parse rules and check pattern matching separately. This means the full `decide()` integration path (parsing config from CLI args + rules + evaluating layers) is NOT tested end-to-end. The tests bypass `parse_config()` which reads from `std::env::args()`.

**P6-2** `hooks/hook_pre_subagent_tool/src/main.rs`
- Allow-direction tests: `read_under_schemas_allowed`, `read_under_docs_allowed`, `write_under_output_allowed`, `no_tool_name_allowed`, `no_path_in_input_allowed`, `grep_with_path_field_under_prefix_allowed`.
- Deny-direction tests: `read_outside_prefix_denied`, `write_outside_prefix_denied`, `unknown_tool_denied`, `path_traversal_denied`, `path_traversal_mid_path_denied`.
- Coverage: Both directions covered. Tests use `decide_with_map()` which is the core logic separated from CLI arg parsing. Good testability design.

**P6-3** `hooks/hook_pre_llm_bash/src/main.rs`
- Allow-direction tests: `benign_ls_not_flagged` (line 609), `benign_cargo_build_not_flagged` (line 620), `benign_cat_file_not_flagged` (line 631), `benign_git_status_not_flagged` (line 642), `benign_git_diff_not_flagged` (line 648).
- Deny-direction tests: `subversion_rm_lock_detected`, `subversion_chflags_noschg_detected`, `truncation_head_claude_md_detected`, `evasion_git_checkout_claude_md_detected`, and many more (20+ detection tests).
- Severity tests: `check_category_block_returns_deny`, `check_category_warn_returns_warn`.
- Exemption tests: `exempted_pattern_returns_none`.
- Coverage: Excellent dual-direction coverage. Both allow and deny paths tested. No violation.

**P6-4** `hooks/hook_post_llm_tool/src/main.rs`
- Tests cover `classify_file()` (both matching and non-matching extensions), `extract_file_path()` (null, missing, numeric).
- No test verifies the full `assess()` function end-to-end (it shells out to `saga` and `syn` binaries which may not be available in test). This is acceptable for subprocess-dependent code.

---

## P7: Schema-First Data Validation

**Rule**: Use JSON Schema validation (via `schema_core`/`schemas_embedded`) for data shape validation. Do not write procedural shape-checking code that duplicates what a schema could enforce.

### Findings

**P7-1** `interceptors/traffic_interceptor_rewriter/src/main.rs` lines 60--116
- `classify_exchange()` performs procedural shape-checking: `value.get("messages")?.as_array()?.len()`, checking for `system`, `tools`, counting tool definitions. This is structural validation of Claude API exchange objects that has no schema backing. If exchange structure matters enough to classify, it should be schema-defined.

**P7-2** `capability/saga_runner/src/lib.rs` lines 109--132
- Ruff JSON output parsing: `item["code"].as_str().unwrap_or("")`, `item["location"]["row"].as_u64().unwrap_or(1)`. This is procedural shape-checking of ruff's JSON output format. While ruff is an external tool, the parsing assumes specific JSON shapes without validation.

**P7-3** `capability/saga_runner/src/lib.rs` lines 154--190
- Basedpyright JSON output parsing: `data["generalDiagnostics"].as_array()`, nested field access. Same issue as P7-2.

**P7-4** `cli/syn_cli/src/main.rs` -- TOML config parsing
- Filter config files (`.syn/warn.toml`, `.syn/deny.toml`) are parsed with manual `toml::from_str` and procedural field extraction. No schema validation of config file structure.

**P7-5** `watchers/watch_and_diff_exchange_intercepts/src/main.rs` lines 190--191
- `serde_json::from_str(trimmed)` on each JSONL line with no schema validation. Exchange objects are parsed as `serde_json::Value` with no structural guarantees.

---

## P8: Stale Tests After Contract Changes

**Rule**: When a contract (schema, API, type) changes, tests that verify the old contract become stale and misleading.

### Findings

**P8-1** `capability/io_check/src/lib.rs`
- No `#[cfg(test)]` module present. This crate has zero tests. The crate provides the `run_check()` entry point used by all 8 `check_*` CLI binaries. A contract change in `io_check` would propagate to all check tools with no test coverage to detect it.

**P8-2** `capability/datagram/src/lib.rs`
- Has 5 tests, but all test `workspace_from_path()` only. Zero tests for `emit()`, `emit_validated()`, `emit_validated_or_alert()`, `try_emit()`, or the transport layer (Unix socket, UDP multicast). The core emission and validation functions -- the crate's primary API -- are completely untested. If the socket path, multicast address, or serialization format changes, no test catches it.

**P8-3** `capability/schemas_embedded/src/lib.rs` tests (lines 154--312)
- Tests only verify `schema_name()` returns the expected string and `schema_json()` is non-empty. They do NOT validate that the schema content is actually valid JSON Schema. If a symlinked schema file changes to invalid JSON, these tests would still pass (the `include_str!` would embed whatever content the symlink points to).
- The actual JSON Schema compilation happens lazily in `schema_core::EmbeddedValidator::get_validator()`, and any error there would `panic!` via `.expect()`. The tests do not exercise `get_validator()`.

---

## P9: Gleipnir Check Accuracy

**Rule**: Gleipnir checks must be calibrated correctly -- false positives erode trust, false negatives miss real issues.

### Findings

**P9-1** `core/gleipnir_core/src/checks_rs/prohibited.rs` -- `check_no_println`
- Lines 228--260: The check exempts `println!` in `main.rs` files and in `print_*`/`emit_*`/`display_*` functions. However, `io_check/src/lib.rs` (a capability crate, NOT a main.rs file) uses `println!` extensively in `emit_result()`, `emit_error()`, and `print_help()` (lines 103--152). If gleipnir runs on io_check, these would be false positives since `print_help` is an output function but `emit_result` and `emit_error` are not in the exemption list (they do not start with `print_`, `emit_`, or `display_`).
- Wait: `emit_result` DOES start with `emit_` -- correction. But `emit_error` also starts with `emit_`. So these would be exempted. `print_help` starts with `print_`. So all would be exempted. However, the standalone `println!` calls inside `print_help()` on lines 137--152 are inside a function that starts with `print_` -- these would be correctly exempted. The `emit_result` on line 103 does `println!()` inside a function starting with `emit_` -- also correctly exempted. No false positive here.

**P9-2** `core/gleipnir_core/src/checks_rs/prohibited.rs` -- `check_no_clone_spam`
- The remaining plans note: "no_clone_spam_IMPROVEMENT.md -- broaden call_expression ownership recognition." This indicates known false positives in clone detection where ownership transfers in call expressions are not fully recognized. This is a documented accuracy gap.

**P9-3** `core/gleipnir_core/src/checks_rs/prohibited.rs` -- `check_no_string_abuse`
- The remaining plans note: "no_string_abuse_IMPROVEMENT.md -- same ownership broadening + error closures." Another documented accuracy gap for string conversion detection.

**P9-4** `core/gleipnir_core/src/checks_rs/style.rs` -- `check_nesting_depth`
- The remaining plans note: "nesting_depth_rs_IMPROVEMENT.md -- remove match_arm from RUST_NESTING_TYPES." This indicates `match` arms are incorrectly counted as nesting levels, producing false positives on idiomatic Rust match expressions.

**P9-5** `core/gleipnir_core/src/checks_rs/prohibited.rs` -- `check_no_println` for main.rs
- The remaining plans note: "no_println_IMPROVEMENT.md -- exempt main() in binaries, output-named functions." Wait, the current code already implements this exemption (lines 230, 249). Let me re-read the plan title: "exempt main() in binaries, output-named functions." The current code exempts `is_binary_main(source.file_path)` (any function in main.rs) and `in_output_function()`. This is broader than "main() in binaries" -- it exempts ALL functions in main.rs, not just `fn main()`. A `fn helper()` in main.rs that uses `println!` would be exempted when perhaps it should not be.
- Actually re-reading the code at line 249: `if name == "println" && (in_binary || in_output_function(...))` -- `in_binary` exempts the entire main.rs file, not just `fn main()`. This means a helper function in main.rs that erroneously uses `println!` instead of `eprintln!` would not be caught. The plan suggests narrowing this to just `fn main()`.

---

## P10: Orphaned Artifacts

**Rule**: Dead references, orphaned files, stale documentation, unused dependencies.

### Findings

**P10-1** Hardcoded user paths in writer binaries
- `writers/write_truth_glossary_record/src/main.rs` line 8: `/Users/johnny/.ai/spaces/bragi/schemas/glossary.schema.json`
- `writers/write_truth_glossary_record/src/main.rs` line 12: `/Users/johnny/.ai/spaces/bragi/truth/quarantine`
- `writers/append_truth_qc_report_record/src/main.rs` line 8: `/Users/johnny/.ai/spaces/bragi/schemas/qc-report.schema.json`
- `writers/append_truth_qc_report_record/src/main.rs` line 12: `/Users/johnny/.ai/spaces/bragi/truth/qc_semantic_report.jsonl`
- `writers/append_embedding_normalize_batch_20/src/main.rs` line 8: `/Users/johnny/.ai/spaces/bragi/definitions/schemas/embedding-target.schema.json`
- `writers/append_embedding_normalize_batch_20/src/main.rs` line 12: `/Users/johnny/.ai/spaces/bragi/interview/embedding_format/normalized.jsonl`
- `writers/append_interview_summaries_record/src/main.rs` line 8: hardcoded path
- `writers/append_interview_summaries_record/src/main.rs` line 12: hardcoded path
- `writers/append_raw_jsonl/src/main.rs` line 8: `/Users/johnny/.ai/smidja/nornir/schemas/tools/raw-jsonl.schema.json`
- `writers/append_raw_jsonl/src/main.rs` line 12: `/Users/johnny/.ai/traffic`
- These are environment-specific absolute paths baked into compiled binaries. If the machine username or directory structure changes, all writers break. The schema_source_path fields are informational (for --help output per write_engine convention), but the output paths are functional.

**P10-2** Schema symlinks pointing to external repositories
- All 20 agent schemas in `schemas/agents/` are symlinks to `/Users/johnny/.ai/smidja/verdandi/agent-builder/output/`. If verdandi moves, all schema symlinks break, and `include_str!()` will fail at compile time.
- 5 tool schemas in `schemas/tools/` are symlinks to `/Users/johnny/.ai/spaces/bragi/` paths. Same breakage risk.
- 1 tool schema symlinks to `/Users/johnny/.ai/smidja/yggdrasil/schemas/`. Same risk.
- This is a deployment dependency, not a code defect per se, but the AUDIT_GUIDE says to report orphaned/stale references.

**P10-3** `schemas_embedded/src/lib.rs` comment at line 8
- Comment says "tools/ -- read/write tools with schema based filter gates (6 schemas)" but there are exactly 6 tool schemas embedded. Comment is accurate. No violation.

**P10-4** `capability/io_check/src/lib.rs` -- no tests
- An untested capability crate is a stale artifact risk. If the contract changes, nothing catches it. (Also noted under P8-1.)

**P10-5** `core/gleipnir_core/src/lib.rs` line 1 docstring
- Says "No I/O -- caller provides source bytes" but `config.rs` performs I/O (P2-1). The docstring is stale/inaccurate.

---

## Summary

| Priority | Findings | Description |
|----------|----------|-------------|
| P1 | 6 | Pure logic in binary crates (syn_cli 1197 lines, interceptor 618 lines, watcher 554 lines, hook_pre_llm_bash 758 lines, dispatcher 388 lines, daemon 379 lines) |
| P2 | 2 | Tier violations: gleipnir_core config.rs has filesystem I/O, format_core paths.rs has env var access |
| P3 | 1 | process::exit() in signal handler (record_datagrams shutdown_handler), plus unsafe static mutation |
| P4 | 5 | JSONL-append pattern duplicated 4x across binaries; exchange classification reimplemented in interceptor vs diff_core |
| P5 | 2 | `datagram` crate lacks tier suffix; `path_verify` lacks tier indicator |
| P6 | 1 | hook_pre_llm_tool decide() integration path not tested end-to-end (tests bypass parse_config) |
| P7 | 5 | Procedural shape-checking without schemas: exchange classification, ruff/pyright output parsing, syn config parsing, JSONL line parsing |
| P8 | 3 | io_check has zero tests; schemas_embedded tests do not verify schema compilation; datagram test coverage unverified |
| P9 | 4 | Documented accuracy gaps in no_clone_spam, no_string_abuse, nesting_depth_rs; no_println exempts entire main.rs instead of just fn main() |
| P10 | 5 | 10+ hardcoded `/Users/johnny/` paths in writers; 26 schema symlinks to external repos; gleipnir_core docstring contradicts config.rs I/O |

**Total findings: 34**
