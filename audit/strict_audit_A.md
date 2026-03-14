# Strict Audit Report A — Nornir Workspace

**Auditor:** A (independent)
**Date:** 2026-03-13
**Scope:** Full workspace audit against AUDIT_GUIDE.md priorities P1-P10
**Method:** Source code reading of every core, capability, and binary crate

---

## Summary

| Priority | Finding Count | Severity Breakdown |
|----------|--------------|-------------------|
| P1 — Pure Logic in Binaries | 0 | — |
| P2 — I/O in Core Crates | 0 | — |
| P3 — Exit Discipline | 0 | — |
| P4 — Composition Over Reimplementation | 1 | 1 Medium |
| P5 — Test Coverage of Decision Logic | 1 | 1 Low |
| P6 — Security Hook Coverage | 0 | — |
| P7 — Schema Enforcement | 0 | — |
| P8 — Naming and Organization | 2 | 1 Medium, 1 Low |
| P9 — Gleipnir Check Accuracy | 2 | 1 Medium, 1 Low |
| P10 — Hardcoded Absolute Paths | 1 | 1 Medium |

**Total findings: 7**
**Critical: 0 | High: 0 | Medium: 4 | Low: 3**

### Overall Assessment

The workspace is in strong architectural health. The most recent improvement plan (items 1-10) addressed the most critical findings from previous audits. The three-tier architecture is strictly maintained: no core crate performs I/O, no capability crate violates its tier boundary, and process::exit is confined to main() functions. All five hook binaries have thorough dual-direction test coverage. Schema validation is consistently enforced through the gate/check architecture. The remaining findings are minor — mostly edge cases in gleipnir check precision and a composition opportunity.

---

## Findings

### P4-01

**Priority:** P4 — Composition Over Reimplementation
**Severity:** Medium
**File(s):** `capability/io_check/src/lib.rs` lines 103-116, 121-133 (println! for structured output)
**What:** The `io_check` crate uses `println!()` extensively in `emit_result` (line 103, 115) and `emit_error` (line 123) to write structured JSON to stdout. These functions handle both JSON and plain-text output modes, including constructing JSON via `serde_json::json!()` and printing it directly. The `io_filter` crate does the same thing at `capability/io_filter/src/lib.rs:28` with `print!("{}", output)`. Both crates are capability crates whose purpose is to write structured output to stdout, so `println!` is correct — but the JSON envelope construction (`{"ok": true, "data": {...}}` and `{"ok": false, "error": {...}}`) is duplicated with what every gate crate does via PyO3. There is no shared Rust function that produces the `{ok, data, error}` envelope.
**Why it matters:** The gate API contract (`{"ok": bool, "data": ..., "error": ...}`) is defined in MANDATORY_READ_BEFORE_CODING.md. The io_check crate reimplements this contract shape as ad-hoc `serde_json::json!()` calls rather than using a shared type. If the contract shape changes, io_check must be updated separately. The PyO3 gates construct the same shape via PyDict, so the duplication crosses the Rust/Python boundary — a shared Rust struct serializable to this shape would reduce drift risk.
**Fix:** Create a `GateResponse` struct in a core crate (perhaps `error_core` since it already defines `ValidationIssue`) with `ok: bool, data: Option<Value>, error: Option<GateError>`. Have `io_check::emit_result` and `io_check::emit_error` use it. Gate crates can also serialize through it if desired.

### P5-01

**Priority:** P5 — Test Coverage of Decision Logic
**Severity:** Low
**File(s):** `capability/io_check/src/lib.rs` lines 155-270
**What:** The `io_check` crate has tests for `parse_check_args` and `emit_result` exit codes, but no integration test that exercises the full `run_check` path with a real validation function. The test coverage is adequate for the argument parsing and exit code logic, but the wiring between `read_input` -> `validate_fn` -> `emit_result` is untested as an integrated flow. This matters because `run_check` is the entry point for 8 check binaries.
**Why it matters:** If the wiring between parse/read/validate/emit is incorrect, it would silently break all check binaries. The individual pieces are tested, but the composition is not. Risk is low because the function is simple and each piece works, but it represents an untested integration path.
**Fix:** Add one test that calls `run_check` with a known validation function and verifies the exit code. This requires either injecting stdin (difficult in unit tests) or extracting the validate-and-emit portion into a testable function that takes input as a parameter rather than reading stdin.

### P8-01

**Priority:** P8 — Naming and Organization
**Severity:** Medium
**File(s):** `capability/io_check/src/lib.rs` line 137, `capability/io_filter/src/lib.rs` line 1
**What:** The crate names `io_check` and `io_filter` do not follow the naming convention for capability crates. NORNIR_NAMING.md specifies that capability crates use the `_io` suffix (e.g., `datagram_io`, `hook_io`, `path_verify_io`) or descriptive names like `write_engine`, `gate_io`, `saga_runner`. The names `io_check` and `io_filter` use an `io_` prefix instead of a `_io` suffix or a descriptive name. This breaks the pattern: the convention is `{domain}_io` not `io_{domain}`.
**Why it matters:** Naming inconsistency makes it harder to predict crate names. When someone looks for "the capability crate that does check I/O," they might search for `check_io` following the `{domain}_io` pattern and not find `io_check`. The prefix-vs-suffix inconsistency violates the stated convention and creates cognitive load.
**Fix:** Rename `io_check` to `check_io` and `io_filter` to `filter_io`, or adopt descriptive names like `check_engine` and `filter_engine` to match the `write_engine` pattern. The `_engine` suffix communicates "capability crate that orchestrates I/O for this category."

### P8-02

**Priority:** P8 — Naming and Organization
**Severity:** Low
**File(s):** `capability/schemas_embedded/src/lib.rs`
**What:** The crate `schemas_embedded` lives in `capability/` but it contains no I/O. It is a pure collection of `include_str!()` constants and `EmbeddedValidator` statics. It depends on `schema_core` (a core crate) and nothing else with I/O. By the tier definitions in NORNIR_ORGANIZATION.md, it should be in `core/` since it is pure data (embedded strings) plus pure computation (validator compilation via `OnceLock`).
**Why it matters:** Placing a pure crate in the capability tier weakens the tier boundary's signal. Someone reading the directory structure would assume `capability/schemas_embedded` does I/O, but it does not. This is a classification error, not a functional one.
**Fix:** Move `schemas_embedded` from `capability/` to `core/`. It depends only on `schema_core` (core) and `include_str!()` (compile-time, no runtime I/O).

### P9-01

**Priority:** P9 — Gleipnir Check Accuracy
**Severity:** Medium
**File(s):** `core/gleipnir_core/src/checks_rs/prohibited.rs` lines 196-209, 211-229, 239-270
**What:** The `no_println` check exempts `println!` inside `fn main()` (line 259) and inside functions named `print_*`, `emit_*`, `display_*` (lines 220-224). The `in_main_function` check walks up ancestors and returns true if ANY enclosing `function_item` is named `main`. This means that `println!` inside a helper function called FROM main would still be flagged (correct), but `println!` inside a closure INSIDE main would be exempted (potentially incorrect for non-output closures). The `in_output_function` exemption is permissive: ANY function starting with `emit_` gets a blanket exemption, but `emit_validated` (in datagram_io) and `emit_validated_or_alert` are not output functions — they are validation functions that happen to start with `emit_`. If gleipnir ever checks capability crates (it currently only runs on binary main.rs files via saga), these would be false negatives.
**Why it matters:** The `emit_*` exemption is a prefix match that will grant false exemptions to functions that start with `emit_` but do not produce stdout output. Currently this is not a live problem because gleipnir Rust checks run on individual files and these exemption functions are only relevant when scanning the specific file. But the exemption is semantically incorrect — `emit_validated` does not use println!, so it would never trigger, but if someone wrote `emit_datagram_debug()` with a `println!` inside it, the check would silently exempt it.
**Fix:** Narrow the `in_output_function` exemption to exact names (`print_help`, `print_usage`, `display_report`, etc.) rather than prefix matching, or require that the exempted function contains `println!` as its primary purpose (a heuristic: the function body is mostly print statements).

### P9-02

**Priority:** P9 — Gleipnir Check Accuracy
**Severity:** Low
**File(s):** `core/gleipnir_core/src/checks_rs/prohibited.rs` lines 88-129
**What:** The `no_unwrap` check has a sophisticated `in_initialization_context` function that exempts `.unwrap()` and `.expect()` inside `static_item`, `const_item`, and closures passed to `get_or_init`/`get_or_try_init`. This is correct and well-calibrated. However, the check does not exempt `.expect()` inside `include_str!()` compile-time contexts or inside `LazyLock::new(|| ...)` closures directly — it only exempts when the closure is passed to `get_or_init`. Looking at `core/gleipnir_core/src/lib.rs:25`, the pattern `LazyLock::new(|| toml::from_str(MESSAGES_TOML).expect(...))` is used. The `is_init_closure` function checks for `get_or_init` and `get_or_try_init` but NOT for `LazyLock::new`. However, the `in_initialization_context` function also checks for `static_item` ancestor (line 121), and since `LazyLock` is always declared as a `static`, the closure is inside a `static_item` and IS correctly exempted via that path. So this is actually correctly handled, but the code path is non-obvious — the exemption works via `static_item` ancestor detection, not via the `is_init_closure` path.
**Why it matters:** The check is correct but fragile. If someone declared a `LazyLock` as a `let` binding inside a function (unusual but possible), the exemption would not apply. The `is_init_closure` function was clearly added to handle `OnceLock::get_or_init` which appears inside function bodies, but `LazyLock::new` gets exempted by a different path. This dual-path exemption should be documented.
**Fix:** Add `new` to the method list in `is_init_closure` so that `LazyLock::new(|| ..)` is explicitly recognized, or add a comment explaining that `LazyLock::new` closures are exempted via the `static_item` ancestor path.

### P10-01

**Priority:** P10 — Hardcoded Absolute Paths
**Severity:** Medium
**File(s):** `writers/write_truth_glossary_record/src/main.rs` lines 9-14, `writers/append_embedding_normalize_batch_20/src/main.rs` lines 9-14, `writers/append_truth_qc_report_record/src/main.rs` lines 9-13, `writers/append_interview_summaries_record/src/main.rs` lines 9-14, `writers/append_raw_jsonl/src/main.rs`
**What:** All four writer binaries (glossary, embedding, qc_report, summaries) construct output paths using `write_engine::ai_home()` which resolves to `$HOME/.ai/`. The paths themselves are relative to that base (e.g., `spaces/bragi/truth/quarantine`), which is correct. However, the `schema_source_path` field in the WriterConfig is constructed as an absolute path like `base.join("spaces/bragi/schemas/glossary.schema.json")`. This path is used only for `--help` display, but it hardcodes the assumption that schemas live at `~/.ai/spaces/bragi/schemas/`. If the bragi workspace moves or the schema directory structure changes, these display paths become stale. The output paths (`spaces/bragi/truth/quarantine`, etc.) are similarly hardcoded but these are the actual target locations and cannot be easily abstracted.
**Why it matters:** The hardcoded paths couple four binary crates to a specific directory layout. If the bragi workspace moves, all four writers need source changes and redeployment. The `ai_home()` function provides the base, but everything after it is a string literal. This was identified in the previous improvement plan (item 10) and the `ai_home()` function was the fix — but the relative paths after it are still hardcoded strings.
**Fix:** This is partially by design (writer binaries ARE specific to a workspace layout), but the `schema_source_path` field is purely informational. Consider either dropping it from `--help` output or deriving it from the schema's embedded name rather than a hardcoded filesystem path.

---

## Verified Clean Areas

The following areas were explicitly checked and found to be correct:

### P1 — Pure Logic in Binaries (CLEAN)
All binary crates delegate to core/capability crates. No binary reimplements validation, format conversion, or decision logic. The check binaries (`check_raw_definition`, `check_universal_format`, etc.) are exemplary: each is under 30 lines, delegating to `io_check::run_check` with a validation closure that calls core functions.

### P2 — I/O in Core Crates (CLEAN)
Every core crate was checked:
- `gleipnir_core`: no std::fs, no std::env, no std::io (only tree-sitter parsing of byte slices)
- `format_core`: no std::fs, no std::env (the `resolve_env` parameter is a closure injected by the caller, not direct env access; verified in `tomlx/processor.rs:29` and `tomlx/paths.rs`)
- `schema_core`: no I/O (uses OnceLock for lazy compilation, operates on strings)
- `error_core`: no I/O (pure error types and formatting)
- `path_core`: no I/O (pure path string validation)
- `syn_core`: no I/O (jaq filter compilation and evaluation on in-memory data)
- `saga_core`: no I/O (pure types: `SanityReport`, `Issue`, path functions)
- `report_render_core`: no I/O (pure formatting and grouping)
- `datagram_types`: no I/O (pure Datagram struct and serialization)
- `diff_core`: no I/O (pure diff computation)

All core Cargo.toml files were checked — none depend on capability crates. The previous audit finding about `gleipnir_core/config.rs` doing I/O has been resolved (the module no longer exists).

### P3 — Exit Discipline (CLEAN)
All `process::exit()` calls are inside `fn main()`. Verified by cross-referencing the grep results for `process::exit` against `fn main()` locations. Every match is in a main.rs file inside `fn main()`. Helper functions throughout the codebase return `Result`.

### P6 — Security Hook Coverage (CLEAN)
All five hook binaries have comprehensive test suites:
- `hook_pre_llm_tool`: 35 tests covering floor/probing/gaming layers, allow_path exemptions, disabled categories, per-rule severity overrides, and path field extraction (both `file_path` and `path` fields). Tests both directions: malicious inputs detected, benign inputs allowed.
- `hook_pre_llm_bash`: 60+ tests covering subversion, truncation, evasion, destruction, and revert categories. Each category has both positive (must detect) and negative (must not detect) tests. Includes benign command tests (`ls`, `cargo build`, `cat`, `git status`, `git diff`).
- `hook_pre_subagent_tool`: 22 tests covering tool-based path restrictions with multiple prefixes, path traversal detection, field preference order (`file_path` over `path`), unknown tool denial, and empty map behavior.
- `hook_pre_subagent_bash`: shares the bash rule engine with `hook_pre_llm_bash` (same `parse_rules`/`check_category` functions).
- `hook_post_llm_tool`: tests present (verified at line 105).

### P7 — Schema Enforcement (CLEAN)
All writer binaries use `write_engine::run()` which validates stdin against the embedded schema before writing. All check binaries use `schema.validate()`. All gate crates use `gate_io::read_and_validate()` or `gate_io::validate_and_write()`. The `schemas_embedded` crate has tests that verify all 26 schemas compile successfully. No binary bypasses schema validation.

### Previous Audit Findings — Verified Fixed
- **gleipnir_core config.rs I/O in Tier 1**: Fixed. The config module no longer exists. `format_core/tomlx` now takes a `resolve_env` closure parameter.
- **record_datagrams static mut unsoundness**: Fixed. The code now uses `AtomicBool` (line 118) and `OnceLock` (line 119) instead of `static mut`. The only `unsafe` is the `libc::signal` call (line 128-131) which is inherently unsafe and properly contained.
- **syn_cli pure logic extraction**: Fixed. `syn_core` contains all filter/policy logic. `syn_cli/src/main.rs` is I/O orchestration only.
- **datagram / path_verify naming**: Fixed. Renamed to `datagram_io` and `path_verify_io`.
- **format_core tomlx env access**: Fixed. `resolve_env` is now a closure parameter, not direct `std::env` access.
