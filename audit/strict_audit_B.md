# Strict Audit B — Nornir Workspace

Date: 2026-03-12
Auditor: Claude Opus 4.6 (strict mode)
Scope: All priorities P1-P10 from AUDIT_GUIDE.md

---

## P1: Pure Logic in Binary Crates

> Rule: Business logic belongs in core or capability crates. Binaries should be thin orchestrators: parse args, call library, handle exit.

### Findings

**P1-01: syn_cli contains entire jq filter engine inline**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 32-59
- `compile_filter()`, `matches_filter()` — jq compile+eval logic is pure computation. This is a reusable filter engine embedded in a binary crate. Should live in a core crate (e.g., `filter_core` or within `report_render_core`).
- Rule violated: "If it has no I/O, it belongs in a core crate."

**P1-02: syn_cli contains config loading logic inline**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 65-99
- `SynConfig`, `load_filter_config()`, `load_config()` — config file parsing and default management. The pattern of reading `.syn/warn.toml` and `.syn/deny.toml` is pure except for `std::fs::read_to_string`. The expression parsing and default handling is testable pure logic embedded in a binary.
- Rule violated: "Pure logic in binaries that could be unit-tested independently."

**P1-03: syn_cli contains three-tier filtering engine inline**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 306-386
- `FilteredOutput`, `is_visible()`, `apply_filters()` — this is the core decision engine for syn. 80 lines of pure computation that decides warn/deny/allow. Should be in a library crate, not inlined in a 493-line main.rs.
- Rule violated: "Logic that makes decisions belongs in libraries."

**P1-04: syn_cli contains broadcast logic inline**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 392-414
- `broadcast()` — constructs datagram and calls `emit_validated_or_alert`. This is a reusable pattern but is embedded in the binary.

**P1-05: hook_pre_llm_tool contains parse_config with manual arg parsing**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/src/main.rs`, lines 50-82
- Manual arg parsing loop. However, this is a deliberate design choice for hooks (lightweight, no clap dependency). Noted but severity is lower given hook constraints.

**P1-06: hook_pre_llm_bash contains make_decision display logic**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/src/main.rs`, lines 191-228
- Command truncation logic (`if command.len() > 60`) is presentation logic embedded in a binary. Could be a shared utility.

### Summary: 6 findings

---

## P2: Three-Tier Dependency Model

> Rule: core/ crates depend only on other core/ crates and workspace deps. capability/ crates depend on core/ crates. Binaries depend on capability/ or core/. No upward dependencies.

### Findings

**P2-01: diff_core (core) depends on datagram_types (core) — acceptable**
- File: `/Users/johnny/.ai/smidja/nornir/core/diff_core/Cargo.toml`, line 13
- This is core-to-core, which is allowed. No violation.

**P2-02: report_render_core (core) depends on saga_core and format_core (both core) — acceptable**
- File: `/Users/johnny/.ai/smidja/nornir/core/report_render_core/Cargo.toml`, lines 13-14
- Core-to-core is allowed.

**P2-03: io_check (capability) uses println! for structured output**
- File: `/Users/johnny/.ai/smidja/nornir/capability/io_check/src/lib.rs`, lines 103-152
- `io_check` is a capability crate that contains 17 `println!` calls. While this is I/O (appropriate for capability tier), the `print_help` function at lines 137-152 has 16 `println!` calls — this is extensive stdout usage that resembles a binary entry point more than a capability library.
- Rule: Capability crates do I/O but should not own user-facing presentation.

**P2-04: datagram (capability) uses libc directly, not through workspace**
- File: `/Users/johnny/.ai/smidja/nornir/capability/datagram/Cargo.toml`, line 12
- `libc = "0.2"` is specified directly rather than through workspace dependencies. All other shared dependencies use `{ workspace = true }`. This is inconsistent.
- Rule violated: Workspace dependency consistency.

**P2-05: intercept_io (capability) depends on format_core (core) — this is correct tier direction but crate-type includes cdylib**
- File: `/Users/johnny/.ai/smidja/nornir/capability/intercept_io/Cargo.toml`, line 8
- `crate-type = ["cdylib", "rlib"]` — this is a PyO3 gate module in the capability tier. The cdylib is necessary for Python FFI but the crate is under `capability/` not `gates/`. If it is a gate, it should be under `gates/`. If it is a capability, cdylib is unexpected.

### Summary: 3 findings (P2-03, P2-04, P2-05)

---

## P3: process::exit and Panic Discipline

> Rule: Only main() calls process::exit(). Helper functions return Result. No .unwrap()/.expect() in production library code.

### Findings

**P3-01: record_datagrams shutdown_handler calls process::exit(0)**
- File: `/Users/johnny/.ai/smidja/nornir/daemons/record_datagrams/src/main.rs`, line 175
- `std::process::exit(0)` inside `extern "C" fn shutdown_handler()`, which is a signal handler — not main(). While this is a signal handler (special case), the AUDIT_GUIDE rule is absolute: "Only main() calls process::exit()."
- Rule violated: process::exit outside main().

**P3-02: gleipnir_core uses .expect() in static initializer**
- File: `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/lib.rs`, line 26
- `toml::from_str(MESSAGES_TOML).expect("gleipnir_messages.toml parse error")` — This is a core crate using `.expect()`. While this is a `LazyLock` static initializer (runs once on first access), the embedded TOML is compile-time constant. The expect will never fire unless someone edits the TOML incorrectly. However, the rule says "All helper functions return Result."
- Rule violated: .expect() in core crate production code.

**P3-03: schema_core uses .expect() in static initializer**
- File: `/Users/johnny/.ai/smidja/nornir/core/schema_core/src/lib.rs`, lines 51, 53
- `.expect("embedded schema must be valid JSON")` and `.expect("embedded schema must be valid JSON Schema")` — same pattern as P3-02. Static initializer with compile-time constants.
- Rule violated: .expect() in core crate production code.

**P3-04: hook_pre_subagent_bash uses Regex::new(...).unwrap() in LazyLock statics**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_subagent_bash/src/main.rs`, lines 22, 26, 30
- Three `Regex::new(...).unwrap()` calls inside `LazyLock` statics. These are compile-time constant regex patterns, so they will never fail. However, these are in a binary crate's module scope, not in main().
- Rule: .unwrap() in production code.

**P3-05: record_datagrams uses static mut and unsafe blocks**
- File: `/Users/johnny/.ai/smidja/nornir/daemons/record_datagrams/src/main.rs`, lines 144-180
- `static mut SHUTDOWN_SOCKET_PATH: Option<PathBuf>` and `static mut SHUTDOWN_FLAG: bool` with 5 `unsafe` blocks. `static mut` is unsound in Rust unless you can guarantee single-threaded access. Signal handlers can interrupt any code, including code that holds partial writes to these statics.
- Rule violated: Panic discipline — unsafe mutable statics are a soundness issue.

**P3-06: datagram crate uses unsafe for signal handling**
- File: `/Users/johnny/.ai/smidja/nornir/capability/datagram/src/lib.rs`, line 121
- `unsafe { libc::signal(...) }` — necessary for signal handling but undocumented in safety comments.

### Summary: 6 findings

---

## P4: Composition Over Reimplementation

> Rule: Use existing crates. Do not reimplement logic that already exists in the workspace.

### Findings

**P4-01: syn_cli reimplements broadcast datagram construction**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 392-414
- Manually constructs a `datagram::Datagram` with hardcoded fields. The same pattern exists in `watch_and_diff_exchange_intercepts`. No shared helper for "build a quality datagram."

**P4-02: format!("syn") and format!("directory") where string literals suffice**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 397, 399
- `source: format!("syn")` and `classifier: Some(format!("directory"))` — these should be `"syn".to_string()` and `Some("directory".to_string())`. `format!()` with no arguments is wasteful.

**P4-03: Multiple binaries have identical error-handling patterns**
- Files: All writer binaries (`append_raw_jsonl`, `write_truth_glossary_record`, `append_truth_qc_report_record`, `append_embedding_normalize_batch_20`, `append_interview_summaries_record`)
- All 5 writer mains have identical `Ok(msg) => println!("{msg}")` / `Err(msg) => { eprintln!("{msg}"); std::process::exit(1); }` pattern. This is already factored via `write_engine::run()` but the error handling wrapper is duplicated.

**P4-04: Manual arg parsing in hook_pre_llm_tool and hook_pre_llm_bash duplicated**
- Files: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/src/main.rs` (lines 50-82), `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/src/main.rs` (lines 79-117)
- Both hooks parse `--category severity` pairs from CLI args with near-identical while-loop patterns. This shared parsing pattern could be extracted to `hook_io::rules`.

### Summary: 4 findings

---

## P5: Naming Encodes Architecture

> Rule: Directory name = package name = binary name. Verb-prefix naming required. Exceptions documented in NORNIR_NAMING.md.

### Findings

**P5-01: io_check name does not follow verb-prefix convention**
- File: `/Users/johnny/.ai/smidja/nornir/capability/io_check/Cargo.toml`
- The capability crate `io_check` does not follow the `verb_noun` convention. It should be something like `check_io` to match the verb-prefix pattern. Other capability crates follow this: `gate_io`, `hook_io`, `write_engine`, `io_filter`.
- Rule violated: Verb-prefix naming.

**P5-02: io_filter name does not follow verb-prefix convention**
- File: `/Users/johnny/.ai/smidja/nornir/capability/io_filter/Cargo.toml`
- `io_filter` puts the noun before the verb-like word. Should be `filter_io` for consistency.
- Rule violated: Verb-prefix naming.

**P5-03: intercept_io is under capability/ but behaves as a gate**
- File: `/Users/johnny/.ai/smidja/nornir/capability/intercept_io/Cargo.toml`
- Has `cdylib` crate-type (PyO3 module). All other PyO3 modules are under `gates/`. If this is a gate, it should be under `gates/`. If it is truly a capability crate, the `cdylib` is misplaced.
- Rule violated: Directory structure encodes architecture.

**P5-04: path_verify naming inconsistency**
- File: `/Users/johnny/.ai/smidja/nornir/capability/path_verify/Cargo.toml`
- Under capability/ which is correct (does I/O — checks filesystem paths). But the name `path_verify` follows `noun_verb` not `verb_noun`. Should be `verify_paths` or similar.
- Rule violated: Verb-prefix naming.

**P5-05: schemas_embedded naming convention**
- File: `/Users/johnny/.ai/smidja/nornir/capability/schemas_embedded/Cargo.toml`
- `schemas_embedded` is a noun-adjective pattern, not verb-prefix. However, this is a data crate (compile-time embedded schemas), not an action crate. The naming convention primarily targets action crates, so this is borderline.

### Summary: 4 findings (P5-01 through P5-04; P5-05 is borderline)

---

## P6: Security Hook Coverage

> Rule: Security hooks must test both directions — malicious input IS detected (no false negatives), benign input is NOT flagged (no false positives).

### Findings

**P6-01: hook_pre_llm_tool tests do not call decide() directly**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/src/main.rs`, lines 199-447
- Tests verify rule parsing and path matching via `parse_rules()` and manual pattern checks, but do NOT test the full `decide()` function with constructed `HookInput` payloads. The `decide()` function calls `parse_config()` which reads CLI args, making it untestable. The test at line 422-432 explicitly acknowledges: "We can't call decide() directly due to parse_config() reading CLI args."
- Rule violated: The core decision function has no integration test. Only its sub-components are tested.

**P6-02: hook_pre_llm_tool does not test gaming category**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/src/main.rs`, tests section
- Tests verify floor rules and probing rules via `parse_rules()`, but there are no tests that exercise gaming-category rules with specific detection patterns (only that `gaming` rules exist in the TOML). No false-negative test for gaming.
- Rule violated: Dual-direction test coverage incomplete for gaming category.

**P6-03: hook_pre_subagent_tool does not test path traversal with encoded variants**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_subagent_tool/src/main.rs`, lines 314-335
- Tests `../` traversal but not URL-encoded variants (`%2e%2e/`), null byte injection, or symlink-through-allowed-prefix attacks. The path traversal check (line 83: `target.contains("..")`) is substring-based and may miss encoded variants.
- Rule violated: Security tests should cover adversarial evasion.

**P6-04: hook_pre_llm_bash does not test env var override via subshell**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/src/main.rs`
- Tests cover `export HOOK_LLM_ALLOW_PATHS` and `env HOOK_LLM_ALLOW_BASH`, but do not test `bash -c 'export HOOK_...'` or `$(export HOOK_...)` subshell wrapping. An LLM could use a subshell to set the env var.

### Summary: 4 findings

---

## P7: Schema-First Data Validation

> Rule: Data shapes should be enforced by JSON Schema, not procedural code. If you find yourself writing `if data.get("field")...`, there should be a schema.

### Findings

**P7-01: hook_io HookInput uses serde_json::Value for tool_input**
- File: `/Users/johnny/.ai/smidja/nornir/capability/hook_io/src/lib.rs`, line 26
- `pub tool_input: serde_json::Value` — the tool_input field is an untyped JSON blob. All hook binaries manually extract fields with `.get("file_path").and_then(|v| v.as_str())`. There is no schema validating the hook input structure.
- Rule violated: Procedural shape-checking instead of schema validation.

**P7-02: syn_cli SynConfig loads TOML without schema validation**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 75-99
- The `.syn/warn.toml` and `.syn/deny.toml` files are parsed manually with `content.parse::<toml::Table>()` and `.get("filter")?.as_str()`. No schema validates the config file structure.
- Rule violated: Config files parsed without schema.

**P7-03: hook_pre_llm_bash / hook_pre_llm_tool rules.toml parsed without schema**
- Files: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/src/main.rs`, `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/src/main.rs`
- Rules TOML files are parsed with `parse_toml_table()` + `parse_rule_array()` — manual field extraction. No JSON Schema validates the rules.toml structure.
- Rule violated: Embedded config parsed without schema.

**P7-04: PostHookInput tool_input is unvalidated serde_json::Value**
- File: `/Users/johnny/.ai/smidja/nornir/capability/hook_io/src/lib.rs`
- Same issue as P7-01 but for PostHookInput. The `tool_input` and `tool_result` fields are untyped.

### Summary: 4 findings

---

## P8: Stale Tests After Contract Changes

> Rule: When a contract changes, tests must be updated to match. Stale tests that pass vacuously are worse than no tests.

### Findings

**P8-01: hook_pre_llm_tool floor/probing/gaming tests do not verify the full rule set**
- File: `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/src/main.rs`, lines 212-232
- Tests check that `/.ssh/`, `/.aws/`, `/.gnupg/` exist in floor rules, and `/.claude/hooks/`, `/.gleipnir/`, `/.claude/settings` exist in probing rules. But if rules.toml adds new floor rules, no test will verify coverage of the new rules. Tests only verify a subset of expected patterns.
- Rule violated: Tests should track contract completeness.

**P8-02: Writer binaries have no tests**
- Files: All 5 writer mains (`append_raw_jsonl`, `write_truth_glossary_record`, `append_truth_qc_report_record`, `append_embedding_normalize_batch_20`, `append_interview_summaries_record`)
- Zero `#[cfg(test)]` modules. The writer binaries are thin wrappers around `write_engine::run()`, but they hardcode schema references, paths, and format choices that are never tested.
- Rule violated: Contract between writer config and write_engine is untested.

**P8-03: sender binaries (send_heartbeat, send_warning, send_notification) have no tests**
- Files: `/Users/johnny/.ai/smidja/nornir/senders/send_heartbeat/src/main.rs`, `send_warning/src/main.rs`, `send_notification/src/main.rs`
- These construct `Datagram` structs with hardcoded field values (e.g., `DatagramKind::Canary` for heartbeat, `Priority::High` for warning) but have no tests verifying the contract between the hardcoded values and the datagram schema.
- Rule violated: Hardcoded contracts untested.

**P8-04: convert_json_to_toml has no tests**
- File: `/Users/johnny/.ai/smidja/nornir/converters/convert_json_to_toml/src/main.rs`
- 60-line binary with no tests. The `run()` function is testable (takes args, returns Result) but untested.
- Rule violated: Testable logic with no tests.

### Summary: 4 findings

---

## P9: Gleipnir Check Accuracy

> Rule: Checks must have zero false positives and minimal false negatives. Every exception path must be tested.

### Findings

**P9-01: nesting_depth_rs includes match_expression but not match_arm**
- File: `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/checks_rs/style.rs`, lines 135-141
- `RUST_NESTING_TYPES` includes `match_expression` but not `match_arm`. A match with 10 arms each containing an if-expression is deeply nested code, but the nesting counter only increments for the `match` itself, not per-arm. This is documented as a known plan item in `plans/nesting_depth_rs_IMPROVEMENT.md` — the plan says to REMOVE `match_arm` from the types, meaning the current behavior is intentional. However, the opposite problem exists: `match_expression` counts as one nesting level, but a `match` inside a `match` inside a `match` would count as 3, which is correct.
- Noted: This is a known accuracy concern with an existing improvement plan.

**P9-02: no_clone_spam considers match_arm as ownership transfer context**
- File: `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/checks_rs/prohibited.rs`, lines 369, 394
- `match_arm` is treated as an ownership transfer context, exempting `.clone()` inside match arms. This can produce false negatives — a `.clone()` in a match arm that could use a reference is exempted.
- Rule violated: False negatives in match arm clone detection.

**P9-03: no_println exemption for main.rs could miss library println in binary crates**
- File: `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/checks_rs/prohibited.rs`, lines 196-198, 249
- `is_binary_main()` checks if `file_path.rsplit('/').next() == Some("main.rs")`. This exempts ALL `println!` in any `main.rs` file, including ones in library modules that happen to be named `main.rs` (unlikely but possible). Also, this exempts println in helper functions within main.rs that are not main() itself.
- The current design (line 249) exempts println in main.rs files OR in output functions (`print_*`, `emit_*`, `display_*`). A `run()` function in main.rs that calls `println!` for user output is correctly exempted, but a `parse_config()` function in main.rs that accidentally uses `println!` is also exempted.

**P9-04: no_unwrap does not check .expect() in match/if-let patterns**
- File: `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/checks_rs/prohibited.rs`, lines 143-183
- The check only looks for `call_expression` with `field_expression` where field is `unwrap` or `expect`. It correctly handles `.unwrap()` method calls. However, it does not detect `Option::unwrap()` or `Result::unwrap()` when called as a free function (though this pattern is rare in Rust).

**P9-05: Rust checks do not have a matrix — checks are hardcoded in run_checks_rust()**
- File: `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/lib.rs`, lines 113-130
- Python checks use a `matrix.rs` with `FileKind`-based dispatch. Rust checks are hardcoded as a flat array in `run_checks_rust()`. There is no Rust-specific FileKind classification (binary vs library, core vs capability). All Rust files get the same checks regardless of their tier.
- Rule violated: Rust checks lack the context-awareness of Python checks.

### Summary: 5 findings

---

## P10: Orphaned Artifacts

> Rule: No dead code, orphaned files, stale references, or unused dependencies.

### Findings

**P10-01: Hardcoded absolute paths in all writer binaries**
- Files: All 5 writer mains under `writers/`
- Paths like `/Users/johnny/.ai/spaces/bragi/schemas/glossary.schema.json` and `/Users/johnny/.ai/spaces/bragi/truth/quarantine` are hardcoded. These are user-specific absolute paths that only work on one machine. If the repository is shared or the user changes their home directory, these paths break.
- Rule violated: Hardcoded paths tied to a specific machine.

**P10-02: schema_source_path in WriterConfig is informational-only**
- File: `/Users/johnny/.ai/smidja/nornir/capability/write_engine/src/lib.rs`, line 66
- `schema_source_path` is documented as "Absolute path to the .schema.json source file (for --help display)." It is only used for display, but it contains absolute paths to files that may not exist at runtime. If the path is wrong, the --help output shows a stale path. No validation that schema_source_path actually exists.

**P10-03: SynConfig has unused _warn_expr and _deny_expr fields**
- File: `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs`, lines 71-72
- Fields `_warn_expr: String` and `_deny_expr: String` are prefixed with `_` to suppress dead-code warnings. These are stored but never read. If they are not needed, they should be removed. If they are for debugging, they should be properly named.
- Rule violated: Dead fields suppressed with underscore prefix.

**P10-04: audit/ directory referenced in MEMORY.md but was empty**
- Directory: `/Users/johnny/.ai/smidja/nornir/audit/`
- MEMORY.md references multiple audit files (`structural_audit.md`, `contract_audit.md`, etc.) but the directory was empty at audit time. The git status shows `audit/` as untracked (`??`), suggesting the directory exists but no audit files have been committed.
- Rule violated: Stale documentation references.

**P10-05: MEMORY.md references plans/ directory with unexecuted plans**
- File: `/Users/johnny/.ai/smidja/nornir/MEMORY.md`
- References 4 plans in `plans/` that are "Not Yet Executed." If these plans exist as files, they are in-progress work. If the plans directory does not exist or is stale, the references are orphaned.

**P10-06: `#[allow(private_interfaces)]` in hook_io response.rs**
- File: `/Users/johnny/.ai/smidja/nornir/capability/hook_io/src/response.rs`, line 1
- `#![allow(private_interfaces)]` is a crate-level suppression. The gleipnir checks explicitly exempt `private_interfaces` (line 66 of suppression.rs), but this is still a compiler warning being suppressed at the crate level rather than at the specific item level.

**P10-07: Only 1 schema file exists under schemas/**
- Directory: `/Users/johnny/.ai/smidja/nornir/schemas/`
- Only `schemas/tools/raw-jsonl.schema.json` was found. The `schemas_embedded` crate embeds multiple schemas (RAW_DEFINITION, GLOSSARY, QC_REPORT, DATAGRAM, etc.) but only one source schema file is under `schemas/`. The others are presumably at external paths like `/Users/johnny/.ai/spaces/bragi/schemas/`. This means schema source-of-truth is scattered across the filesystem.
- Rule violated: Schema source files not co-located in the repository.

### Summary: 7 findings

---

## Audit Summary

| Priority | Description | Findings |
|----------|-------------|----------|
| P1 | Pure Logic in Binary Crates | 6 |
| P2 | Three-Tier Dependency Model | 3 |
| P3 | process::exit and Panic Discipline | 6 |
| P4 | Composition Over Reimplementation | 4 |
| P5 | Naming Encodes Architecture | 4 |
| P6 | Security Hook Coverage | 4 |
| P7 | Schema-First Data Validation | 4 |
| P8 | Stale Tests After Contract Changes | 4 |
| P9 | Gleipnir Check Accuracy | 5 |
| P10 | Orphaned Artifacts | 7 |
| **Total** | | **47** |

### High-Severity Findings (by impact)

1. **P3-05**: `static mut` with unsafe blocks in `record_datagrams` — soundness issue. Signal handler can race with main thread access to `SHUTDOWN_FLAG` and `SHUTDOWN_SOCKET_PATH`.
2. **P1-01/02/03**: syn_cli has ~200 lines of pure computation logic that should be in a library crate. This is the largest binary and the most architecturally significant P1 violation.
3. **P6-01**: The core `decide()` function in `hook_pre_llm_tool` has no integration test. Only sub-components are tested. This is a security hook.
4. **P9-05**: Rust checks lack FileKind-based dispatch. All Rust files get identical checks regardless of whether they are core libraries, capability crates, or binary entry points.
5. **P10-01/07**: Hardcoded absolute paths in writer binaries and scattered schema source files make the repository non-portable.
6. **P8-02/03/04**: 10 binary crates have zero tests.
