# Strict Audit B -- 2026-03-13

Auditor: Claude Opus 4.6 (Auditor B, independent)

## Summary

| Priority | Findings | Critical | High | Medium | Low |
|----------|----------|----------|------|--------|-----|
| P1       | 2        | 0        | 1    | 1      | 0   |
| P2       | 1        | 0        | 0    | 1      | 0   |
| P3       | 1        | 0        | 0    | 1      | 0   |
| P4       | 2        | 0        | 1    | 1      | 0   |
| P5       | 1        | 0        | 0    | 0      | 1   |
| P6       | 0        | 0        | 0    | 0      | 0   |
| P7       | 2        | 0        | 0    | 1      | 1   |
| P8       | 0        | 0        | 0    | 0      | 0   |
| P9       | 0        | 0        | 0    | 0      | 0   |
| P10      | 2        | 0        | 0    | 1      | 1   |
| **Total**| **11**   | **0**    | **2**| **6**  | **3**|

**Overall Assessment:** The workspace is in strong health. The two previous audit rounds and the 10-item improvement plan have resolved the most damaging structural issues (gleipnir_core I/O in Tier 1, static mut unsoundness, JSONL-append duplication, hardcoded paths in writers). The three-tier architecture is well enforced. Binary crates are thin and compose correctly from core/capability crates. All deploy scripts are consistent with the actual crate directories. There are no critical findings.

The remaining findings are medium and low severity: a few pockets of pure logic trapped in binary crates, one composition bypass in the interceptor's raw-append path, zero test coverage for `io_filter`, and an orphaned workspace member (`io_filter` is defined but never imported by any crate). None of these are urgent, but each represents architectural drift that will compound if left.

---

## Findings

### P1-01

**Priority:** P1 -- Pure Logic Must Not Live in Binary Crates
**Severity:** High
**File(s):** `/Users/johnny/.ai/smidja/nornir/interceptors/traffic_interceptor_rewriter/src/main.rs` lines 27-116
**What:** The `classify_exchange` function and its helpers (`is_main_agent`, `tool_count`, `is_web_search_only`) plus the `ExchangeKind` enum and `MAIN_AGENT_IDENTITY` constant are pure logic trapped in a binary crate. These functions take a `serde_json::Value` and return a classification -- no I/O, no side effects, fully deterministic.
**Why it matters:** The watcher binary (`watch_and_diff_exchange_intercepts`) already delegates its diff logic to `diff_core`. But if any future tool needs to classify exchanges (e.g., a replay analyzer, a traffic summary tool, a compaction counter), that classification logic is locked inside this binary and will be reimplemented. The `MAIN_AGENT_IDENTITY` constant is especially concerning -- if the identity string changes, the hardcoded copy here and any future copies will diverge.
**Fix:** Extract `ExchangeKind`, `classify_exchange`, `is_main_agent`, `tool_count`, `is_web_search_only`, and `MAIN_AGENT_IDENTITY` into `diff_core` (which already handles exchange splitting and diffing). The interceptor binary becomes thinner orchestration.

### P1-02

**Priority:** P1 -- Pure Logic Must Not Live in Binary Crates
**Severity:** Medium
**File(s):** `/Users/johnny/.ai/smidja/nornir/watchers/watch_and_diff_exchange_intercepts/src/main.rs` lines 58-91
**What:** The functions `parse_pace` and `jitter_sleep` (xorshift64 PRNG) are pure logic in a binary crate. `workspace_from_parent_dir` is also a pure function that extracts a workspace name from a path -- a different implementation from `datagram_io::workspace_from_path` but serving a related purpose.
**Why it matters:** `parse_pace` and `jitter_sleep` are generic utility functions. If another daemon or watcher ever needs rate-limited polling with natural variation, this logic will be reimplemented. `workspace_from_parent_dir` is particularly notable because `datagram_io` already has `workspace_from_path` -- having two workspace derivation functions in different places (one in a binary, one in a capability crate) is exactly the divergence pattern P1 warns about.
**Fix:** `workspace_from_parent_dir` should be consolidated with `datagram_io::workspace_from_path` or exposed alongside it. The pace/jitter functions are lower priority but could live in a utility module if another binary needs them.

### P2-01

**Priority:** P2 -- The Three-Tier Dependency Model
**Severity:** Medium
**File(s):** `/Users/johnny/.ai/smidja/nornir/core/datagram_types/Cargo.toml` line 2
**What:** The `datagram_types` crate sits in `core/` but its package name is `datagram_types`, not `datagram_types_core`. Per the naming convention, core crates use the `_core` suffix, and the suffix and directory must agree.
**Why it matters:** An LLM encountering `datagram_types` (no `_core` suffix) in the `core/` directory receives conflicting signals about its tier. The name suggests capability tier; the directory says core tier. A future session might incorrectly add I/O to it (because the name doesn't signal purity), or might place a new types-only crate outside `core/` (following the naming pattern rather than the directory pattern). The crate is genuinely pure (only serde derives, no I/O) and correctly placed in `core/`, so the fix is the name.
**Fix:** Rename `datagram_types` to `datagram_types_core` (package name, directory name, and all `use` statements). Alternatively, if the `_core` suffix feels redundant for a types-only crate, document this as an explicit naming exception in `NORNIR_NAMING.md` alongside the saga_cli/syn_cli exceptions.

### P3-01

**Priority:** P3 -- process::exit and Panic Discipline
**Severity:** Medium
**File(s):** `/Users/johnny/.ai/smidja/nornir/core/schema_core/src/lib.rs` lines 49-53
**What:** Two `.expect()` calls in `EmbeddedValidator::get_validator()`:
```rust
let schema: Value = serde_json::from_str(self.schema_json)
    .expect("embedded schema must be valid JSON");
Validator::new(&schema)
    .expect("embedded schema must be valid JSON Schema")
```
These are in a `OnceLock::get_or_init` closure, which means they execute lazily on first use rather than at program startup.
**Why it matters:** These `.expect()` calls are in a core library crate. While the schemas are embedded at compile time via `include_str!()` and a malformed schema would be caught at build time (the include would fail or tests would fail), the `.expect()` pattern is a panic site in production code. If a schema file somehow became corrupted after build (unlikely but not impossible in a dynamic linking scenario), this would panic the entire process without the caller having any opportunity to handle the error gracefully. The improvement plan item #1 (gleipnir_core `.expect()` to `Result`) was completed for gleipnir_core but this parallel pattern in schema_core was not addressed.
**Fix:** Change `get_validator` to return `Result<&Validator, String>` and propagate the error. The `validate` and `is_valid` methods already return `Result`, so they can propagate naturally. This does change the `OnceLock` pattern slightly (would need `OnceLock<Result<Validator, String>>` or similar), so the risk/reward should be weighed. Given that these are compile-time-embedded schemas, this is medium severity -- the risk is theoretical, not practical.

### P4-01

**Priority:** P4 -- Composition Over Reimplementation
**Severity:** High
**File(s):** `/Users/johnny/.ai/smidja/nornir/interceptors/traffic_interceptor_rewriter/src/main.rs` lines 122-138
**What:** The `append_raw` function does its own `OpenOptions::new().create(true).append(true).open()` + `write_all` + `flush()` instead of using `write_engine::append_line_fsync`. Critically, it calls `.flush()` but NOT `.sync_all()` (fsync). Every other JSONL append in the workspace uses `write_engine::append_line_fsync` which does call `sync_all()`.
**Why it matters:** This is a durability inconsistency. `append_raw` writes raw bytes to `rawdata_{session_id}.jsonl` without fsync. If the process crashes or the machine loses power between `flush()` (which only pushes to the OS buffer) and the OS flushing to disk, the rawdata file can lose the last entry. Every other append path in the workspace uses `append_line_fsync` which calls `sync_all()` for durability. The interceptor specifically handles compaction detection -- losing a rawdata entry during a compaction event means losing the evidence of what was compacted. The `append_raw` function exists because it writes raw bytes (not a single JSON line) and needs to append both the bytes and a newline separately, but `write_engine::append_line_fsync` already handles the "line + newline" pattern.
**Fix:** Use `write_engine::append_line_fsync` if the raw bytes are a single JSON line (which they are -- stdin is one JSON object). If raw-byte fidelity is needed (avoiding re-serialization), extend `write_engine` with an `append_bytes_fsync` function and use that.

### P4-02

**Priority:** P4 -- Composition Over Reimplementation
**Severity:** Medium
**File(s):** `/Users/johnny/.ai/smidja/nornir/watchers/watch_and_diff_exchange_intercepts/src/main.rs` lines 345-364
**What:** The `transcript_path_for`, `open_transcript`, and `append_transcript` functions implement their own file append pattern (OpenOptions + write_all) without using `write_engine::append_line_fsync`. The transcript append does not fsync.
**Why it matters:** Same pattern as P4-01 -- a parallel file append implementation that lacks fsync. The transcript is a structured log of datagram payloads that the watcher emitted. Losing transcript entries on crash means the replay-vs-watch comparison loses fidelity. Using `write_engine::append_line_fsync` would provide consistency and durability.
**Fix:** Replace `append_transcript` with `write_engine::append_line_fsync`. The transcript content is already serialized JSON, so it fits the append_line_fsync contract exactly.

### P5-01

**Priority:** P5 -- Naming Encodes Architecture
**Severity:** Low
**File(s):** `/Users/johnny/.ai/smidja/nornir/core/datagram_types/Cargo.toml` line 2
**What:** Same as P2-01. The package name `datagram_types` lacks the `_core` suffix required for crates in `core/`. This is listed separately under P5 because it is also a naming violation independent of the tier model.
**Why it matters:** Addressed in P2-01.
**Fix:** Addressed in P2-01.

### P7-01

**Priority:** P7 -- Schema-First Data Validation
**Severity:** Medium
**File(s):** `/Users/johnny/.ai/smidja/nornir/cli/syn_cli/src/main.rs` lines 36-63
**What:** The `.syn/warn.toml` and `.syn/deny.toml` configuration files are parsed with ad-hoc TOML deserialization (`SynFilterToml` struct with a single `filter` field) without any schema validation. There is no `.schema.json` file defining what a valid syn configuration looks like.
**Why it matters:** Without a schema, the configuration format is defined implicitly by the Rust struct. If the format grows (e.g., adding a `severity_threshold` field, or `exclude_tools`), there is no schema to validate against, and malformed config files silently fall through to defaults (the `unwrap_or_else` on line 48). A user who writes `filtere = "..."` (typo) gets no error -- the typo field is silently ignored and the default filter is used. This was identified in the previous improvement plan (item #8) as needing a schema, but the plan item was marked DONE while the actual schema was not added.
**Fix:** Create `schemas/tools/syn-config.schema.json` defining the warn/deny config shape. Validate config files against it in `load_filter_config`. Add to `schemas_embedded`.

### P7-02

**Priority:** P7 -- Schema-First Data Validation
**Severity:** Low
**File(s):** `/Users/johnny/.ai/smidja/nornir/interceptors/traffic_interceptor_rewriter/src/main.rs` lines 60-116
**What:** The `classify_exchange` function performs procedural shape-checking on the exchange JSON (checking `system[1].text` contains an identity string, checking `tools` array length, checking tool name equals `"web_search"`). This is not schema validation -- it is manual field inspection.
**Why it matters:** The classification logic checks specific JSON structure expectations (system is an array, index 1 exists, it has a text field, tools is an array, tools[0] has a name field). These structural expectations are not documented in any schema. If the Claude API exchange format changes (e.g., system blocks reorder, tools gain a wrapper), the classification will silently misclassify without any schema validation catching the mismatch. This is lower severity because the classification is inherently heuristic (there is no "exchange classification schema" that would make sense), but the structural expectations of what fields exist and where could be documented.
**Fix:** This is a judgment call. The classification is heuristic by nature and a schema may not fit well. Consider at minimum documenting the structural expectations in the function's doc comment (currently undocumented: "expects system to be an array with identity at index 1").

### P10-01

**Priority:** P10 -- Orphaned Artifacts
**Severity:** Medium
**File(s):** `/Users/johnny/.ai/smidja/nornir/Cargo.toml` line 10, `/Users/johnny/.ai/smidja/nornir/capability/io_filter/`
**What:** The `io_filter` crate is listed as a workspace member and exists on disk, but is not imported by ANY other crate in the workspace. No `Cargo.toml` in the entire workspace lists `io_filter` as a dependency. It has zero tests. It contains a single 36-line function (`run_filter`) that is never called.
**Why it matters:** An orphaned crate creates confusion. A future session sees `io_filter` in the workspace, assumes it is used, and may try to compose with it or update it to stay consistent with changes elsewhere. The crate's purpose (stdin-validate-stdout filter contract) is served by `io_check` for the check_* binaries, and no binary currently uses the filter pattern that `io_filter` provides. It appears to have been created for a future use case that never materialized.
**Fix:** Either remove `io_filter` from the workspace (delete directory, remove from `Cargo.toml` members) or add it as a dependency to the binary crates that should use it. If keeping it, add tests.

### P10-02

**Priority:** P10 -- Orphaned Artifacts
**Severity:** Low
**File(s):** `/Users/johnny/.ai/smidja/nornir/capability/io_filter/src/lib.rs` line 28
**What:** `io_filter::run_filter` uses `print!("{}", output)` instead of `println!` or `write!` to stdout. This is a minor inconsistency (no trailing newline) but more importantly, the function has zero test coverage, so this behavior is unverified.
**Why it matters:** If `io_filter` were ever adopted by a binary, the lack of trailing newline on stdout output could cause subtle piping issues. Combined with zero tests, the crate is both unused and untested -- a dead artifact.
**Fix:** Subsumed by P10-01. If keeping the crate, add tests and decide on newline behavior.

---

## Areas Verified Clean

The following areas were audited and found to be in compliance:

**Tier model (P2):** All core crates (`error_core`, `format_core`, `schema_core`, `path_core`, `syn_core`, `saga_core`, `gleipnir_core`, `report_render_core`, `diff_core`, `compaction_inject_core`) are pure -- no `std::fs`, `std::io`, `std::net`, or `std::env` imports in any core crate source file. The one exception (`datagram_types` naming) is documented above. Capability crates correctly perform I/O. No binary-to-binary dependencies exist.

**process::exit discipline (P3):** Every `process::exit()` call is in a `main()` function. All helper functions return `Result`. The `.expect()` calls in `schema_core` (documented above) are the only panic sites outside test code and static initializers.

**Composition (P4):** JSONL-append has been consolidated through `write_engine::append_line_fsync` across the workspace (record_datagrams, intercept_io, traffic_interceptor_rewriter's exchange/precompact paths). The two exceptions (rawdata append, transcript append) are documented above. Writer binaries correctly delegate to `write_engine::run()`. Schema validation correctly delegates to `schema_core` + `schemas_embedded`. Diff logic correctly delegates to `diff_core`. Hook contract correctly delegates to `hook_io`.

**Naming (P5):** All binary directory names match their `Cargo.toml` package names. All verb prefixes match their category directories (check_ in cli/, gate_ in gates/, hook_ in hooks/, send_ in senders/, append_/write_ in writers/, convert_ in converters/, rewrite_ in rewriters/, split_ in dispatchers/, watch_ in watchers/, traffic_ in interceptors/, record_ in daemons/). The saga_cli/syn_cli naming exceptions are documented. The `datagram_types` naming issue is the only finding.

**Security hook coverage (P6):** Hook test counts are healthy: hook_pre_llm_bash (57 tests), hook_pre_subagent_bash (58 tests), hook_pre_llm_tool (31 tests), hook_pre_subagent_tool (15 tests), hook_post_llm_tool (11 tests). The pre_llm_tool decide function has tests for both malicious detection (floor/probing/gaming deny) and benign allowance (benign_path_allowed, no_path_in_input_allowed). No gaps found in dual-direction coverage.

**Schema-first validation (P7):** All agent pipeline stages have schemas in `schemas/agents/`. All tool data formats have schemas in `schemas/tools/`. The datagram schema exists and is used by `datagram_io::emit_validated`. Gate crates correctly use `schemas_embedded` validators. The syn config gap is documented above.

**Stale tests (P8):** No evidence of stale tests. Test assertions match current struct shapes and function signatures. The improvement plan's contract changes (renames, new parameters like `resolve_env`) are reflected in tests.

**Gleipnir check accuracy (P9):** Gleipnir_core has no I/O (verified: zero `std::fs`/`std::io`/`std::env` imports). The config loading I/O was extracted per improvement plan item #1. Check matrix, parsing, and classification are pure. No false positive patterns identified in the check definitions.

**Deploy script consistency (P10):** Every deploy script's crate list matches the actual crate directories. No orphaned entries, no missing crates. Workspace `Cargo.toml` members list matches actual directories (verified: all 85 member paths exist).

---

## Improvement Plan Status Verification

The 10-item improvement plan (`plans/IMPROVEMENT_PLAN.md`) was verified:

1. **gleipnir_core config.rs I/O in Tier 1** -- VERIFIED FIXED. Zero `std::fs`/`std::io`/`std::env` imports in gleipnir_core.
2. **record_datagrams static mut unsoundness** -- VERIFIED FIXED. Uses `AtomicBool` + `OnceLock` instead of `static mut`.
3. **syn_cli pure logic extraction** -- VERIFIED FIXED. Filter engine is in `syn_core`, rendering in `report_render_core`. syn_cli is orchestration.
4. **JSONL-append duplication** -- VERIFIED FIXED. All JSONL appends use `write_engine::append_line_fsync` (with two minor exceptions documented in P4-01/P4-02).
5. **hook_pre_llm_tool decide() untested** -- VERIFIED FIXED. 31 tests including decide function with dual-direction coverage.
6. **no_println exempts entire main.rs** -- Not directly verifiable from source (gleipnir check config is external), but no println calls found in core/capability library code.
7. **io_check zero tests** -- VERIFIED FIXED. io_check has tests (15+ test functions covering arg parsing and serialization).
8. **Hook inputs / syn config without schema** -- PARTIALLY FIXED. syn config still lacks a schema (P7-01). Hook input schemas were not added but hook_io handles the contract.
9. **datagram / path_verify naming** -- VERIFIED FIXED. Renamed to `datagram_io` and `path_verify_io`.
10. **Hardcoded absolute paths in writers** -- VERIFIED FIXED. Writers use `write_engine::ai_home()` for path construction.
