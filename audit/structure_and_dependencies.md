# Nornir Structural Audit

Date: 2026-03-11
Scope: Priorities 1, 2, 4, 5, 8

## Summary

11 violations found: 3 critical, 8 notable

---

## Priority 1: Pure Logic in Binaries

### syn (cli/syn/src/main.rs) -- 1179 lines -- CRITICAL

The `syn` binary contains a complete jaq-based filter engine (`compile_filter`, `matches_filter`, `apply_filters`) totaling approximately 120 lines of pure logic that operates on data structures (no I/O). This is a reusable filter-and-decide pipeline that could serve other tools. If the `syn` binary were deleted, this jq-filter-over-issues capability would disappear.

Trapped pure functions:
- `compile_filter()` -- compiles a jq expression string into a filter
- `matches_filter()` -- evaluates a compiled filter against an Issue
- `apply_filters()` -- runs warn/deny filters across reports, returns decision

These should live in a `filter_core` or be merged into `report_render_core`.

### split_jsonl_batches (dispatchers/split_jsonl_batches/src/main.rs) -- 529 lines -- NOTABLE

Contains `compute_batches()` (lines 39-67), a pure algorithm for optimal batch size distribution. This is a reusable algorithm with no I/O. Tests cover it thoroughly. If deleted, this algorithm disappears. However, at 29 lines it is below the 50-line threshold for non-orchestration logic, and only one consumer exists. Low priority.

### traffic_interceptor_rewriter (interceptors/traffic_interceptor_rewriter/src/main.rs) -- 681 lines -- NOTABLE

Contains pure classification functions (`is_main_agent`, `tool_count`, `is_web_search_only`, `classify_exchange`) totaling approximately 50 lines. These are explicitly marked `// Classification -- pure` in the source. If deleted, this classification logic disappears. However, this is tightly coupled to the interceptor's specific protocol and unlikely to be reused elsewhere. Low priority.

### record_datagrams (daemons/record_datagrams/src/main.rs) -- 426 lines -- NOTABLE

Contains a hand-rolled `today()` date formatter (lines 93-130, 38 lines). This is pure logic (no I/O) that computes civil dates from Unix timestamps. Reimplements functionality that could be shared. See also Priority 4.

### All other binaries -- CLEAN

- **check_* binaries** (20-27 lines each): Trivial orchestration wrappers. CLEAN.
- **writers** (22-23 lines each): Thin wrappers on write_core + schemas_embedded. CLEAN.
- **senders** (23-26 lines each): Thin wrappers on datagram. CLEAN.
- **hooks**: hook_pre_llm_tool (447 lines), hook_pre_llm_bash (758 lines), hook_pre_subagent_bash (748 lines), hook_pre_subagent_tool (334 lines), hook_post_llm_tool (201 lines) -- all delegate decision logic through `hook_io::run_hook(decide)`. The `decide` functions are hook-specific policy that reads config, parses rules from embedded TOML, and makes decisions. This is orchestration-level code that appropriately lives in the binary. The reusable rule parsing (`parse_rule_array`, `parse_severity`, `parse_toml_table`, `RawRule`, `Severity`) correctly lives in `hook_io::rules`. CLEAN.
- **rewrite_compaction_summary** (315 lines including tests): Thin wrapper on compaction_inject_core. CLEAN.
- **watch_and_diff_exchange_intercepts** (585 lines): Orchestration over diff_core + datagram. CLEAN.
- **convert_json_to_toml** (92 lines): Thin wrapper on format_core. CLEAN.
- **saga** (207 lines): Thin wrapper on saga_core. CLEAN.

---

## Priority 2: Tier Violations

### CRITICAL: diff_core depends on datagram (core depends on capability)

`core/diff_core/Cargo.toml` has:
```
datagram = { path = "../../capability/datagram" }
```

This is a tier violation. `diff_core` is a core crate (Tier 1) but depends on `datagram`, a capability crate (Tier 2). The three-tier model requires core crates to only depend on other core crates and external dependencies.

The dependency exists because `diff_core::build_datagram()` constructs a `datagram::Datagram` struct and uses `datagram::Priority` and `datagram::DatagramKind` types. The types (`Datagram`, `Priority`, `DatagramKind`) could be split into a `datagram_types_core` crate (Tier 1), while the I/O transport (`emit`, `try_emit`, `try_unix_stream`, `try_udp_multicast`) stays in the capability crate.

### CRITICAL: datagram capability depends on schemas_embedded capability

`capability/datagram/Cargo.toml` has:
```
schemas_embedded = { path = "../schemas_embedded" }
```

Capability-to-capability dependencies are architecturally allowed, but this creates a problematic coupling: the transport layer (`datagram`) depends on the validation layer (`schemas_embedded`), which depends on ALL agent pipeline schemas. Any schema change triggers recompilation of every datagram consumer.

### All other dependency checks -- CLEAN

**Core crates -- all CLEAN:**
- `error_core`: external deps only (serde, serde_json, jsonschema, thiserror)
- `format_core`: external deps + error_core (core-to-core)
- `schema_core`: external deps + error_core (core-to-core)
- `path_core`: external deps + error_core (core-to-core)
- `write_core`: external deps + error_core + schema_core (core-to-core)
- `gleipnir_core`: external deps only
- `saga_core`: external deps + gleipnir_core (core-to-core)
- `report_render_core`: external deps + saga_core + format_core (core-to-core)
- `compaction_inject_core`: serde_json only

**Capability crates -- all CLEAN (except datagram noted above):**
- `gate_io`: core deps (error_core, format_core, schema_core) + capability dep (path_verify)
- `hook_io`: external deps + datagram (capability-to-capability, allowed)
- `intercept_io`: external deps + format_core (capability-to-core, allowed)
- `io_check`: external deps + error_core (capability-to-core, allowed)
- `io_filter`: error_core only (capability-to-core, allowed)
- `path_verify`: error_core + path_core (capability-to-core, allowed)
- `schemas_embedded`: schema_core (capability-to-core, allowed)

**Binary crates -- all CLEAN:**
No binary depends on another binary. All binaries depend only on core and capability crates.

---

## Priority 4: Composition Violations

### Arg parsing duplication across 7+ binaries -- NOTABLE

The following binaries each contain a hand-rolled `parse_args()` function with the same pattern (manual `while i < args.len()` loop, match on flag strings, return Config struct):

- `split_jsonl_batches` (lines 73-156)
- `watch_and_diff_exchange_intercepts` (lines 32-86)
- `traffic_interceptor_rewriter` (lines 199-241)
- `record_datagrams` (lines 43-79)
- `rewrite_compaction_summary` (lines 31-54)
- `send_datagram` (lines 24-87)
- `convert_json_to_toml` (lines 30-60)

This is a repeated pattern, not shared logic. Each parser is specific to its binary's flags, so extracting a shared crate would add complexity without reducing duplication. The pattern is idiomatic for zero-dependency Rust CLIs. **Not a violation** -- this is acceptable structural repetition.

### today() reimplemented in record_datagrams -- NOTABLE

`daemons/record_datagrams/src/main.rs` lines 93-130 hand-rolls a civil date formatter (38 lines of epoch-to-YYYY-MM-DD conversion). The `datagram` capability crate has `now()` for Unix timestamps but no date formatting. If another binary needs dates, this will be reimplemented again. Consider a `time_core` or adding `today()` to an existing core crate.

### No reimplementation of existing core/capability logic found -- CLEAN

All binaries correctly delegate to:
- `write_core` + `schemas_embedded` for validated JSONL writing
- `datagram` for datagram emission
- `hook_io` for hook protocol handling
- `diff_core` for exchange diffing
- `compaction_inject_core` for compaction injection
- `format_core` for format conversion
- `saga_core` + `report_render_core` for quality analysis
- `gate_io` for gate protocol handling

---

## Priority 5: Naming Violations

### All core crates have _core suffix -- CLEAN

`error_core`, `format_core`, `schema_core`, `path_core`, `write_core`, `gleipnir_core`, `saga_core`, `report_render_core`, `diff_core`, `compaction_inject_core` -- all correct.

### Binary verb prefixes -- CLEAN

- `check_*` binaries: validation CLI tools (correct verb)
- `append_*` / `write_*` writers: enforcement output tools (correct verb)
- `send_*` senders: datagram emission tools (correct verb)
- `hook_*` hooks: hook intercept binaries (correct prefix)
- `split_*` dispatcher: batch splitting (correct verb)
- `convert_*` converter: format conversion (correct verb)
- `rewrite_*` rewriter: request rewriting (correct verb)
- `watch_*` watcher: file tailing (correct verb)
- `record_*` daemon: persistent logging (correct verb)
- `traffic_interceptor_rewriter`: compound name but accurately describes function

### saga and syn package name vs binary name mismatch -- NOTABLE

`cli/saga/Cargo.toml`: package name is `saga_cli`, binary name is `saga`.
`cli/syn/Cargo.toml`: package name is `syn_cli`, binary name is `syn`.

The workspace Cargo.toml member path is `cli/saga` and `cli/syn`, matching the directory. The package names use `_cli` suffix to avoid Cargo name collisions. The `[[bin]]` name matches the deployed symlink. This is a deliberate naming strategy, not a violation. **Acceptable.**

### datagram capability crate -- NOTABLE (missing suffix)

`capability/datagram` has package name `datagram` without a capability-related suffix. The AUDIT_GUIDE says capability crates don't need a specific suffix, but the name `datagram` is ambiguous -- it could be mistaken for a core crate defining types or a binary. Consider `datagram_io` or `datagram_transport` to signal it performs I/O.

### No generic names found -- CLEAN

No crates named `processor`, `handler`, `manager`, or `utils`.

---

## Priority 8: Orphaned Artifacts

### CRITICAL: 15+ documentation files reference "socket_emit" (renamed to "datagram")

The capability crate `socket_emit` was renamed to `datagram`. All Rust source and Cargo.toml files have been updated, but documentation files still reference the old name:

- `QUICKSTART.md` (2 references)
- `PLAN.md` (3 references)
- `NORNIR_BUILDING_AND_COMPOSITION.md` (5 references)
- `NORNIR_NAMING.md` (1 reference)
- `NORNIR_ORGANIZATION.md` (3 references)
- `cli/syn/SYN_DESIGN.md` (2 references)
- `hooks/HOOK_DESIGN.md` (1 reference)
- `AUDIT_GUIDE.md` (1 reference)
- `audit/code_quality_and_architecture.md` (3 references)
- `audit/naming_and_organization.md` (2 references)

### Missing deploy scripts for interceptors and daemons -- NOTABLE

Deploy scripts exist for: converters, dispatchers, gates, hooks, rewriters, senders, tools, watchers, writers (9 scripts).

Missing deploy scripts for:
- `interceptors/traffic_interceptor_rewriter` -- no `deploy_interceptors.py`
- `daemons/record_datagrams` -- no `deploy_daemons.py`

Both binaries have valid symlinks in `~/.ai/tools/bin/`, so they were deployed manually. The deploy scripts serve as documentation and reproducibility infrastructure.

### schemas/tools/datagram.schema.json -- NOTABLE (unreferenced symlink)

`schemas/tools/datagram.schema.json` is a symlink to `/Users/johnny/.ai/smidja/yggdrasil/schemas/datagram.schema.json`. This file is NOT referenced by any Rust code. The `schemas_embedded` crate uses `validate.datagram.schema.json` (a different file in the same directory). The `datagram.schema.json` symlink appears to be an orphan -- possibly the original schema before `validate.datagram.schema.json` was created.

### Workspace Cargo.toml members -- CLEAN

All 84 workspace members point to existing directories.

### Schema symlinks -- CLEAN

All 27 schema symlinks (20 agents, 7 tools) resolve to existing files.

### ~/.ai/tools/bin/ symlinks -- CLEAN

All 30 nornir-pointing symlinks in `~/.ai/tools/bin/` resolve to existing binaries.
