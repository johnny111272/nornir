# Nornir Code Quality and Architecture Audit

Auditor: Odin (automated)
Date: 2026-03-08
Scope: All 80 workspace crates (9 core, 7 capability, 28+ gates, 7 cli, 5 hooks, 5 senders, 5 writers, 1 watcher, 1 converter, 1 dispatcher, 1 rewriter)

---

## Executive Summary

Nornir is an exceptionally well-architected workspace. The layered tier system (Tier 1 core -> Tier 2 capability -> Tier 3 binaries) is consistently enforced. Pure/impure separation is a first-class concern. Schema discipline is strong. The codebase exhibits clear evidence of intentional design rather than accidental accretion.

### Critical Systemic Issues

1. **Duplication across hook crates** -- Severity/Config/parse_severity/make_decision are reimplemented identically in hook_pre_llm_tool, hook_pre_llm_bash, hook_pre_subagent_tool, and hook_pre_subagent_bash. This shared pattern belongs in hook_io.

2. **syn duplicates qa_core** -- The syn CLI reimplements CheckGroup, group_issues, formatting helpers, and text wrapping that already exist in qa_core. This is a significant parallel implementation.

3. **process::exit scattered in non-main functions** -- Several Tier 3 crates call process::exit() from helper functions rather than returning Result and letting main decide. Worst offenders: send_datagram, rewrite_compaction_summary, syn, saga.

4. **No tests in any Tier 3 binary crate** (except split_jsonl_batches which has excellent tests). Zero test coverage for hooks, senders, writers, watchers, converters, rewriters.

5. **append_raw_jsonl does not use write_core** -- It reimplements the same stdin-read-validate-append-fsync pattern that write_core provides, bypassing schema validation entirely.

### Systemic Strengths

- Tier 1 core crates are almost universally well-tested with pure function separation
- The write_core/io_check/io_filter/hook_io/gate_io capability crates provide excellent reusable contracts
- Schema validation via schemas_embedded is consistently used
- The gate crates are extremely uniform in pattern (good)
- Error types are structured (NornirError) not string-based at Tier 1/2
- Dependencies flow correctly downward through tiers

---

## Tier 1: Core Library Crates

### error_core -- CLEAN

Pure type definitions. NornirError enum with Display impl. No I/O. Has tests. Single responsibility. Exactly what a core error crate should be.

### format_core -- CLEAN

Well-decomposed into modules: convert.rs (strip_nulls, merge), parse.rs (JSON/TOML parsing), serialize.rs (JSON/TOML/TOON output). All functions are pure transformations. Extensive test coverage. The toml_to_json convenience function is a clean composition. The tomlx submodule adds substantial pure TOML processing. This is the most complex core crate and it handles its complexity well through modular decomposition.

### schema_core -- CLEAN

Pure schema validation wrapper around jsonschema crate. SchemaRef struct with compile-time embedding support. ValidationResult is clean typed data. Well-tested. Single responsibility.

### path_core -- CLEAN

Pure path manipulation and validation. resolve_path, is_safe_path, etc. Well-tested. No I/O despite the name (correctly placed in core).

### write_core -- MINOR ISSUES

Good design: WriterConfig struct drives the entire writer contract. The run() function handles stdin reading, schema validation, output routing, fsync. This is the capability that the 4 declarative writers depend on.

Issues:
- process::exit() is called inside run() rather than returning Result. This is acceptable for a CLI harness function, but it means the logic cannot be tested in-process.
- The function is ~200 lines combining multiple responsibilities (arg parsing, stdin reading, validation, writing). Could benefit from decomposition into smaller functions.

### saga_core -- MINOR ISSUES

Core library for quality truth recording. Defines SanityReport, Issue, and the generate_report pipeline. Delegates to gleipnir_core for custom checks and shells out to ruff/basedpyright.

Issues:
- Shells out to external tools (ruff, basedpyright) from a "core" crate. This is impure I/O but necessary for the domain. The impurity is isolated to specific functions (run_ruff, run_basedpyright) which is the right pattern.
- load_qa_file reads from disk -- properly impure, but could be in a capability crate rather than core.
- No tests for the report generation pipeline itself (the constituent gleipnir_core checks are well-tested).

### gleipnir_core -- CLEAN

Excellent architecture. Pure check functions organized by category (architecture, imports, prohibited, style, suppression, type_safety). Each check module has its own comprehensive test suite. The classify module determines file classification. Config loading is separated from check logic. Matrix module provides the check dispatch table. This is the highest-quality crate in the workspace.

### diff_core -- CLEAN

Pure diff computation. Well-tested. Single responsibility.

### qa_core -- MINOR ISSUES

Defines SanityReport type, grouping logic, and three formatters (Colored, Plain, Json). Good trait-based design with QaFormatter.

Issues:
- SanityReport is defined here AND re-exported from saga_core. The canonical location is ambiguous.
- The formatting logic is substantial (~300 lines of terminal formatting). This is appropriate for a core crate since it's pure string transformation.

---

## Tier 2: Capability Crates

### schemas_embedded -- CLEAN

Compile-time schema embedding using include_str! and lazy_static SchemaRef instances. Tests verify all schemas load and compile. Single responsibility. This is the schema registry for the entire workspace.

### path_verify -- CLEAN

Filesystem path verification: walks a JSON schema to find path-typed fields, then checks each path exists on disk. Pure schema walking + impure exists() check. Well-tested.

### io_filter -- CLEAN

Minimal stdin->validate->stdout contract. 36 lines. Perfect single-responsibility capability.

### io_check -- CLEAN

Minimal file->validate->report contract for check_* CLI tools. Clean separation of concerns.

### gate_io -- CLEAN

PyO3 gate contract: read_and_validate, validate_and_write, read_validate_write. Properly composes format_core, schema_core, and path_verify. Has tests. Clean API surface.

### hook_io -- CLEAN

Defines HookInput/PostHookInput deserialization, HookDecision enum (Allow/Warn/Deny), and the run_hook harness. The response module handles JSON output formatting. Clean typed error handling. The harness function properly confines process::exit to the right boundary.

### socket_emit -- CLEAN

Fire-and-forget Unix socket emitter. Datagram struct with serde serialization. 73 lines. try_emit returns Result, emit swallows errors (correct for fire-and-forget). The now() and workspace_name() helpers are useful but slightly overloaded for a "socket" crate -- they are convenience impure functions.

### intercept_io -- CLEAN

PyO3 module exposing json_to_toml and append_jsonl_line to Python. Properly delegates to format_core. Thin boundary layer. 65 lines.

---

## Tier 3: Binary Crates

### hooks/hook_pre_llm_tool -- MINOR ISSUES

Well-structured layered rule engine (floor/probing/gaming). Rules loaded from embedded TOML. Config parsed from CLI args. Decision logic is clean with proper fall-through.

Issues:
- Severity enum, parse_severity(), and make_decision() are duplicated from hook_pre_llm_bash.
- Rule struct and parse_rules() are duplicated across all hook crates.
- No tests.

### hooks/hook_pre_llm_bash -- MINOR ISSUES

Same quality as hook_pre_llm_tool but for bash commands. Uses regex for pattern matching. Three categories: subversion, truncation, evasion.

Issues:
- Same Severity/Config/make_decision duplication.
- Regex compilation happens on every invocation (rules are re-parsed from TOML each time). For a hook called on every bash command, this is a performance concern. Should compile once.
- No tests for the regex patterns.

### hooks/hook_post_llm_tool -- MINOR ISSUES

Clean dispatcher pattern: classify file kind, route to assessment function. Shells out to saga and syn for Python quality assessment.

Issues:
- Binary resolution (tools_bin/saga_bin/syn_bin) uses HOME env var with /tmp fallback -- fragile.
- No tests.

### hooks/hook_pre_subagent_tool -- CLEAN

Clean path prefix validation with good error messages. Uses hook_io properly. Simple and focused.

Issues:
- No tests (the logic is simple enough that manual testing may suffice, but property-based tests on path matching would add value).

### hooks/hook_pre_subagent_bash -- MINOR ISSUES

Most complex hook. Validates writer heredoc patterns and inspect command paths. Good shell safety checks (chaining detection).

Issues:
- Regex::new() called inside parse_heredoc_header on every invocation. Should be lazy_static or OnceCell.
- has_chain_chars is naive -- does not handle quoted strings containing ; or &&. Acceptable for this security context (false positives are safe).
- No tests for a fairly complex parser.

### senders/send_alert -- CLEAN

Trivially correct. 25 lines. Constructs Datagram, calls emit_datagram. Clean use of socket_emit.

Issue: process::exit(1) on arg count mismatch is fine for CLI.

### senders/send_warning -- CLEAN

Same pattern as send_alert. 25 lines.

### senders/send_notification -- CLEAN

Same pattern. 22 lines.

### senders/send_heartbeat -- CLEAN

Same pattern. 22 lines.

### senders/send_datagram -- NEEDS WORK

The flexible datagram sender. 103 lines. Manual arg parsing, schema validation via schemas_embedded.

Issues:
- process::exit() called from 6 different helper closures (unwrap_or_else blocks). The function should return Result<(), Error> and let main call process::exit once.
- Manual arg parsing reimplements a pattern that appears in multiple crates. No shared arg parsing utility.
- The pattern `args.get(i).cloned()` after incrementing i could panic if args is exhausted -- though the while loop condition prevents this, the pattern is fragile.
- No tests despite non-trivial parsing logic.

### writers/append_raw_jsonl -- NEEDS WORK

Standalone JSONL appender that does NOT use write_core.

Issues:
- Duplicates write_core's stdin-read, JSON-validate, append-with-fsync pattern manually.
- No schema validation -- any JSON object is accepted. This bypasses the schema discipline that the other 4 writers enforce.
- Should either be rebuilt on write_core (with a "no schema" option) or clearly documented as the intentional unvalidated writer.
- No tests.

### writers/append_truth_qc_report_record -- CLEAN

Declarative writer: 16 lines of config. Everything delegated to write_core. Schema-validated. This is the ideal writer pattern.

### writers/write_truth_glossary_record -- CLEAN

Same declarative pattern. 17 lines. Clean.

### writers/append_embedding_normalize_batch_20 -- CLEAN

Same declarative pattern with batch_size. 16 lines. Clean.

### writers/append_interview_summaries_record -- CLEAN

Same declarative pattern with DirectoryPrefix output. 17 lines. Clean.

### watchers/watch_and_diff_exchange_intercepts -- MINOR ISSUES

Reads intercept JSONL, diffs consecutive exchanges, emits datagrams. Good functional decomposition: diff_messages, diff_system, diff_tools are pure functions. build_detail is pure.

Issues:
- --watch mode is stubbed ("not yet implemented"). Dead code path.
- extract_workspace does brittle string parsing of system prompt blocks to find workspace name. If the format changes, this silently returns "unknown".
- extract_session_id does similarly brittle parsing of metadata.user_id.
- process::exit in replay's error paths rather than returning Result.
- No tests despite having testable pure functions (diff_messages, diff_system, diff_tools).

### converters/convert_json_to_toml -- CLEAN

Properly delegates to format_core for the actual conversion. Adds atomic file writing (write to .tmp, rename). Clean error handling.

Issues:
- process::exit in write_atomic rather than returning Result. Minor.

### dispatchers/split_jsonl_batches -- CLEAN

The best-tested Tier 3 crate. compute_batches is a pure function with 12 unit tests covering edge cases. parse_args has 6 tests including security checks (path traversal). run() returns Result properly. Main is thin. Path traversal protection on directory names. This crate is the model for how Tier 3 binary crates should be written.

### rewriters/rewrite_compaction_summary -- MINOR ISSUES

Reads JSON from stdin, injects compaction instructions as a system block, writes to stdout. Clean separation: read_stdin, parse_json, inject_system_block, serialize_json.

Issues:
- process::exit scattered in 5 different functions instead of returning Result.
- Debug snapshot writing uses SystemTime directly rather than delegating to a shared timestamp utility.
- No tests.

### cli/check_raw_definition -- CLEAN

20 lines. Delegates to io_check and schemas_embedded. The ideal check tool pattern.

### cli/check_paths_resolved -- CLEAN

Same pattern. 20 lines.

### cli/check_paths_verified -- CLEAN

Same pattern plus path_verify. 27 lines.

### cli/check_universal_format -- CLEAN

Same pattern plus path_verify. 27 lines.

### cli/check_permissions_resolved -- CLEAN

Same pattern. 27 lines.

### cli/saga -- MINOR ISSUES

CLI frontend for saga_core. Two modes: file (single report) and directory (batch sidecar generation). Clean arg parsing.

Issues:
- process::exit in parse_args and run_directory rather than returning Result.
- walk_python_files duplicates the pattern also found in syn's walk_qa_files. Could share a generic directory walker.
- No tests for CLI arg parsing.

### cli/syn -- NEEDS WORK

The most complex Tier 3 crate at 920 lines. Quality policy gate with jq-based filtering, three output modes (TOON/Colored/JSON), and Hlidskjalf broadcast.

Issues:
- **Major duplication**: CheckGroup, LocatedIssue, group_issues(), total_issues(), collapse_line_numbers(), wrap_adaptive(), and the entire colored formatting section are reimplemented from qa_core. The comment "ported from qa_core" confirms this is known duplication.
- format_colored is 145 lines of terminal formatting logic that should live in a shared formatter.
- process::exit called in 7+ locations through helper functions.
- No tests despite complex filtering logic (severity ordering, three-tier filter application, jq compilation).
- The main function is 60 lines handling multiple concerns (input loading, filtering, formatting, broadcasting, exit codes).

### cli/qa_report -- CLEAN

Clean CLI frontend for qa_core. Properly uses qa_core's formatter trait. File discovery logic is well-structured.

Issues:
- walk_for_sidecars duplicates the same directory walking pattern as saga and syn. Minor.

### gates/* (28 crates) -- CLEAN

All gate crates follow an identical pattern:
- Input gates: use gate_io::read_and_validate + specific schema from schemas_embedded
- Output gates: use gate_io::validate_and_write + specific schema
- Passthrough gates: use gate_io::read_validate_write

Each gate is 40-50 lines of boilerplate PyO3 bindings. The pattern is extremely uniform which is good for maintenance. The boilerplate is an acceptable cost given each gate compiles to a separate Python extension module.

---

## Cross-Cutting Analysis

### Pure/Impure Separation

**Rating: STRONG**

Core crates (format_core, gleipnir_core, diff_core, error_core, path_core, schema_core) are genuinely pure -- no filesystem access, no network, no clocks. Impurity is correctly pushed to capability and binary crates.

Exceptions:
- saga_core shells out to ruff/basedpyright (impure by necessity, isolated correctly)
- qa_core re-exports saga_core types (pure itself, but the dependency chain is questionable)

### Schema Discipline

**Rating: STRONG**

All structured output (datagrams, QA reports, gate validations, writer records) goes through schema validation via schemas_embedded + schema_core. The single exception is append_raw_jsonl which intentionally bypasses schemas.

send_datagram validates against the DATAGRAM schema before emitting. The 4 declarative writers validate against their respective schemas. Gates validate on both input and output.

### Error Handling

**Rating: GOOD at Tier 1/2, WEAK at Tier 3**

Tier 1 uses NornirError enum (typed data). Tier 2 capability crates return Result<T, NornirError>. This is correct.

Tier 3 binary crates frequently call process::exit() from non-main functions. This prevents testability and violates the "errors bubble up, main decides" principle. The worst offenders:
- send_datagram: 6 exit points in helpers
- syn: 7+ exit points
- rewrite_compaction_summary: 5 exit points
- write_core::run(): 25 exit points (this is especially problematic since it's a Tier 2 harness)

### Test Coverage

**Rating: EXCELLENT at Tier 1, ABSENT at Tier 3**

Tier 1 core crates have comprehensive test suites:
- gleipnir_core: tests in every check module + classify + parsing
- format_core: tests in convert, parse, serialize, all tomlx submodules
- schema_core: validation tests
- error_core: display tests
- path_core: path resolution tests
- diff_core: diff computation tests

Tier 2 capability crates:
- schemas_embedded: schema compilation tests
- gate_io: integration tests
- path_verify: verification tests
- write_core: writer contract tests

Tier 3 binary crates:
- split_jsonl_batches: 12 tests (exemplary)
- All other Tier 3 crates: ZERO tests

### Code Duplication

Three specific duplication clusters:

1. **Hook duplication**: Severity enum, parse_severity, make_decision duplicated across 4 hook crates. Estimated 80-100 lines of identical code. Should be in hook_io.

2. **syn vs qa_core**: CheckGroup, group_issues, formatting (~200 lines) reimplemented in syn. Should import from qa_core or extract to a shared crate.

3. **Directory walking**: walk_python_files (saga), walk_qa_files (syn), walk_for_sidecars (qa_report) all implement the same recursive directory walk with the same skip patterns. Should be a shared utility in path_core or a new capability crate.

### Dependency Direction

**Rating: CLEAN**

No circular dependencies. No upward dependencies (Tier 3 never depends on another Tier 3 crate). Gate crates depend only on gate_io and schemas_embedded. Writers depend only on write_core and schemas_embedded. Hooks depend only on hook_io. The tier system is rigorously enforced.

---

## Crate Ratings Summary

| Rating | Crates |
|--------|--------|
| CLEAN | error_core, format_core, schema_core, path_core, gleipnir_core, diff_core, schemas_embedded, path_verify, io_filter, io_check, gate_io, hook_io, socket_emit, intercept_io, send_alert, send_warning, send_notification, send_heartbeat, convert_json_to_toml, split_jsonl_batches, check_raw_definition, check_paths_resolved, check_paths_verified, check_universal_format, check_permissions_resolved, qa_report, all 28 gate crates, append_truth_qc_report_record, write_truth_glossary_record, append_embedding_normalize_batch_20, append_interview_summaries_record |
| MINOR ISSUES | write_core, saga_core, qa_core, hook_pre_llm_tool, hook_pre_llm_bash, hook_post_llm_tool, hook_pre_subagent_bash, watch_and_diff_exchange_intercepts, rewrite_compaction_summary, saga (cli) |
| NEEDS WORK | send_datagram, append_raw_jsonl, syn (cli) |
| REBUILD | (none) |

---

## Recommended Actions (Priority Order)

1. **Extract shared hook utilities into hook_io**: Severity, parse_severity, make_decision, Rule, parse_rules. Eliminates 4-way duplication.

2. **Unify syn and qa_core**: Either syn imports from qa_core, or extract the shared types (CheckGroup, group_issues) into a new grouping module in qa_core that both syn and qa_report use.

3. **Rebuild append_raw_jsonl on write_core**: Either add a "no schema" mode to write_core or clearly document why this crate intentionally bypasses validation.

4. **Add tests to Tier 3 crates**: Priority targets are the hooks (security-critical) and syn (complex filtering logic). split_jsonl_batches is the model to follow.

5. **Refactor process::exit out of helper functions**: Return Result up to main. This enables testing and follows the principle that only main decides the exit code.

6. **Extract shared directory walker**: The walk_python_files/walk_qa_files/walk_for_sidecars pattern should be a generic function in path_core or a new utility.
