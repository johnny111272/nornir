# Nornir Build Plan — Syn Quality Pipeline

**Created:** 2026-03-05
**Status:** Phase 0 DONE, Phase 1 DONE (syn rewrite + report_render_core extraction), Phase 2 IN PROGRESS

---

## Context Refresh Pointers

If starting a new session or recovering from compaction, read these files in order:

1. `cli/syn_cli/SYN_DESIGN.md` — full syn design spec
2. This file (`PLAN.md`) — execution plan and task status
3. `core/format_core/src/lib.rs` — format_core (JSON, YAML, TOML, TOON, TOMLX — 74 tests)
4. `core/saga_core/src/lib.rs` — saga library (SanityReport + Issue pure types — 6 tests)
5. `capability/saga_runner/src/lib.rs` — saga I/O (report generation, directory walker, sidecar I/O)
6. `core/report_render_core/src/lib.rs` — QA report grouping/formatting for consumers — 38 tests
7. `cli/syn_cli/src/main.rs` — syn CLI (478 lines, filter engine + orchestration — 43 tests)
8. `Cargo.toml` — workspace members list

**Key design decisions (don't re-derive these):**
- saga_core is pure types only (SanityReport, Issue, path functions). I/O lives in capability/saga_runner.
- syn uses `--mode [report|gate]` not subcommands. Default: report.
- Gate mode is fully deterministic — rejects --tool/--level/--filter flags
- Output: TOON default (LLM), --colored (tty auto-detect), --json (machine)
- Broadcast to Hlidskjalf ON by default, --silent to suppress
- Filtering via jaq-core (pure Rust jq), configs are jq expressions
- .syn/warn.toml = noise filter, .syn/deny.toml = enforcement threshold
- Default warn filter: `.tool == "gleipnir"`, default deny: `.severity == "blocked"`
- format_core rebuild is PREREQUISITE — all tools use it

---

## CLI Interface

```
syn [options] [path]

Modes:
  --mode report    Informational (default). CLI overrides allowed.
  --mode gate      Deterministic policy gate. Locked to config files.
                   Rejects --tool/--level/--filter. Used by hooks.

Input:
  <path>           File (.py finds sidecar, .qa direct), directory, or omit for cwd
  --stdin          Read .qa JSON from stdin (pipe from saga)
  --project-dir    Project root (hooks provide this)
  --target         [src|tests|all] Subtree scope (default: src)

Output:
  (default)        TOON when piped, --colored when tty
  --colored        ANSI terminal output
  --json           Machine-readable JSON
  --silent         Suppress Hlidskjalf broadcast

Filters (report mode only, rejected in gate mode):
  --tool           [gleipnir|ruff|basedpyright|all]
  --level          [info|warning|error|blocked] and above
  --filter         '<jq expression>'
```

---

## Phase 0: format_core Rebuild

**Goal:** Full format conversion matrix as shared nornir infrastructure.

**What exists:**
- `core/format_core/` — TOML↔JSON only, used by all gate checkers
- `core/error_core/` — FormatError types (TomlParse, JsonParse, Conversion)

**What we need:**
- JSON, YAML, TOML, TOON parse/serialize
- TOMLX full parser (unit conversion, path expansion, type declarations)
- Bidirectional conversion matrix between all formats
- Educational error diagnostics on failure
- Existing gate checkers must keep working (they use toml_to_json)

### Tasks

#### 0.1 Add workspace dependencies
- [ ] Add `serde_yaml` to `[workspace.dependencies]` in root Cargo.toml
- [ ] Add `toon-format` to `[workspace.dependencies]`
- [ ] Add both to `core/format_core/Cargo.toml` dependencies
- [ ] Verify workspace builds clean

**Context refresh:** Read `Cargo.toml` root for current workspace deps

#### 0.2 Extend error_core
- [ ] Read current `core/error_core/src/lib.rs`
- [ ] Add YamlParse, ToonParse, TomlxParse error variants
- [ ] Add Educational error variant (from phoenix pattern)
- [ ] Verify existing gate checkers still compile

**Context refresh:** Read `core/error_core/src/lib.rs`

#### 0.3 Add parsers (format string → serde_json::Value)
- [ ] Add `parse_json()` — serde_json::from_str
- [ ] Add `parse_yaml()` — serde_yaml::from_str → Value
- [ ] Add `parse_toon()` — toon_format decode → Value
- [ ] Keep existing `toml_to_json` working (don't break interface)
- [ ] Add `parse_toml()` as the lower-level toml::Value → json::Value
- [ ] Tests for each parser + error cases

**Reference:** `~/.ai/phoenix/rust/io/transformers/core/src/parse.rs`

#### 0.4 Add serializers (serde_json::Value → format string)
- [ ] Add `to_json()` — serde_json::to_string_pretty
- [ ] Add `to_yaml()` — serde_yaml::to_string
- [ ] Add `to_toml()` — json Value → toml Value → toml::to_string_pretty
- [ ] Add `to_toon()` — toon_format encode
- [ ] Tests for each serializer

**Reference:** `~/.ai/phoenix/rust/io/transformers/core/src/serialize.rs`

#### 0.5 Conversion matrix with educational errors
- [ ] Port the "fast path first, diagnostics on failure" pattern
- [ ] json↔yaml, json↔toml, json↔toon, yaml↔toml, yaml↔toon, toml↔toon
- [ ] Educational diagnostics for TOML failures (null values, top-level arrays)
- [ ] Roundtrip tests for all format pairs

**Reference:** `~/.ai/phoenix/rust/io/transformers/core/src/convert.rs`
**Reference:** `~/.ai/phoenix/rust/io/transformers/core/src/diagnostics/`

#### 0.6 TOMLX parser
- [ ] Read TOMLX spec: `~/.ai/phoenix/reverse_sandbox/spec/TOMLX_SPEC.md`
- [ ] Read existing implementation: `~/.ai/phoenix/rust/io/transformers/core/src/tomlx/`
- [ ] Port section annotation parsing
- [ ] Port unit conversion (time, size families)
- [ ] Port path expansion (base, home, absolute, config, env)
- [ ] Port type declarations and JSON Schema generation
- [ ] Port educational error messages (orphan annotations, unknown units, etc.)
- [ ] Full test suite

**This is the biggest single task.** May need its own sub-plan.

#### 0.7 Verify backward compatibility
- [ ] All existing gate checkers (`check_raw_definition`, `check_paths_verified`, etc.) still build
- [ ] All existing gate checkers still pass their tests
- [ ] `gate_io` still works with format_core

**How to verify:** `cargo build --workspace 2>&1` then `cargo test -p format_core`

---

## Phase 1: Syn Core

**Goal:** Working syn binary with jq filtering, TOON output, Hlidskjalf broadcast.

**Prerequisite:** Phase 0 complete (format_core with TOON support) ✓

### CRITICAL DECISIONS MADE (don't re-derive):
- **qa_core and qa_report are deleted.** Grouping/formatting/severity logic extracted to `core/report_render_core/` (38 tests). syn, svalinn, and future QA consumers import from report_render_core.
  - saga_core's `SanityReport` and `Issue` are the canonical pure types
  - saga_runner provides shared directory walking (`walk_files`, `find_files`) and all I/O
  - report_render_core provides `group_issues`, `format_output`, `severity_rank`, `OutputMode`
- **jaq-interpret 1.5 + jaq-parse 1.0** (stable, NOT beta jaq-core 3.0)
  - Core-only (no jaq-std needed) — handles `==`, `or`, `and`, field access
  - API: `ParseCtx::new(Vec::new())` → `jaq_parse::parse(expr, jaq_parse::main())` → `defs.compile(filter)` → `filter.run((Ctx::new([], &inputs), Val::from(json_value)))`
  - Returns `Vec<Result<Val, _>>` — check first result for `Val::Bool(true)`
  - Already added to workspace deps and syn Cargo.toml
- **syn added to workspace members** in root Cargo.toml

### What exists NOW:
- `cli/syn_cli/src/main.rs` — 478 lines, fully functional report + gate modes, 43 tests
- `cli/syn_cli/Cargo.toml` — deps: saga_runner, report_render_core, datagram, jaq-interpret, jaq-parse
- `cli/syn_cli/SYN_DESIGN.md` — full design spec
- `core/report_render_core/src/lib.rs` — extracted grouping/formatting library, 38 tests

### Phase 1 COMPLETE — syn rewrite done:

#### 1.1 jaq dependency — **DONE**
- [x] jaq-interpret 1.5 + jaq-parse 1.0 added to workspace and syn deps
- [x] Proof of concept at /tmp/jaq_test/ — all filter patterns work
- [x] Core-only (no jaq-std) handles all our filter needs

#### 1.2–1.8 Combined: Rewrite syn main.rs
Write as one cohesive binary with these components:

**Args parsing:**
- `--mode [report|gate]` (default: report)
- `--stdin` for piped .qa JSON
- `--project-dir <dir>`, `--target [src|tests|all]`
- `--colored`, `--json`, `--silent`
- `--tool`, `--level`, `--filter` (report mode only)
- Gate mode rejects override flags with clear error

**Filter engine (embed in syn, not a separate crate):**
- Compile jq expressions at startup via jaq-interpret
- `fn compile_filter(expr: &str) -> Result<Filter, String>`
- `fn matches_filter(issue: &serde_json::Value, filter: &Filter) -> bool`
- Three-tier cascade: all → warn filter → deny filter

**Config loading:**
- `.syn/warn.toml` with `filter = '...'` key, default `.tool == "gleipnir"`
- `.syn/deny.toml` with `filter = '...'` key, default `.severity == "blocked"`
- Fail fast on bad jq syntax at load time

**Input handling:**
- File path → find .qa sidecar via saga_core::qa_path()
- Directory → walk for .qa files (reuse walk logic from old code)
- Stdin → read .qa JSON, deserialize as saga_core::SanityReport
- No args → cwd + --target scope

**Grouping (port from qa_core into syn):**
- `group_issues()` → `Vec<CheckGroup>` (group by tool+code)
- `CheckGroup`, `LocatedIssue` structs
- `total_issues()` helper

**Output formatting (port from qa_core into syn):**
- ColoredFormatter — ANSI terminal (port from qa_core, it's good)
- TOON output — serialize grouped JSON via format_core::serialize::to_toon()
- JSON output — serialize via format_core::serialize::to_json()
- TTY auto-detect: check if stdout is a terminal

**Broadcast:**
- datagram::emit() ON by default
- --silent suppresses
- Payload: JSON with decision, issues, groups, workspace

**Exit codes:** 0 (allow/warn), 1 (deny in gate mode), 2 (usage error)

---

## Phase 2: Deploy + Integrate

**Goal:** Syn deployed, wired into hooks, tested end-to-end.

### Tasks

#### 2.1 Deploy
- [ ] Add `("syn_cli", "syn")` to `deploy_tools.py` TOOL_CRATES
- [ ] Release build: `./deploy_tools.py`
- [ ] Verify: `syn --help`, `syn report ~/.ai/gleipnir`

#### 2.2 End-to-end testing
- [ ] Single file: `syn report structures.py`
- [ ] Directory batch: `syn report --project-dir ~/.ai/gleipnir --target src`
- [ ] Stdin pipe: `saga structures.py | syn report --stdin`
- [ ] No args: `cd ~/.ai/gleipnir && syn report`
- [ ] Gate mode: `syn --mode gate --project-dir ~/.ai/gleipnir`
- [ ] Gate rejects flags: `syn --mode gate --tool ruff` → error
- [ ] Default filters (no .syn/): gleipnir only shown
- [ ] Custom warn.toml: verify filter changes output
- [ ] Broadcast: Hlidskjalf receives events
- [ ] --silent: Hlidskjalf does NOT receive events

#### 2.3 Hook integration
- [ ] Wire syn into PostToolUse (replace current saga-only hook)
- [ ] PostToolUse: `syn --mode gate --project-dir $DIR --stdin` (saga pipes in)
- [ ] Verify additionalContext gets TOON output
- [ ] Verify Hlidskjalf gets broadcast

#### 2.4 Future: PreToolUse gate
- [ ] Receive file content from tool_input
- [ ] Call saga with content → .qa JSON
- [ ] Pipe to syn --mode gate --stdin
- [ ] Return permissionDecision based on exit code
- [ ] Trial blocking for new .py writes only

---

## Status Tracking

| Phase | Task | Status |
|-------|------|--------|
| 0.1 | Workspace deps | **done** |
| 0.2 | error_core extend | **done** |
| 0.3 | Parsers | **done** |
| 0.4 | Serializers | **done** |
| 0.5 | Conversion matrix | **done** |
| 0.6 | TOMLX parser | **done** (7 files, 42 tests) |
| 0.7 | Backward compat | **done** (all gate checkers build) |
| 1.1 | jaq deps | **done** (jaq-interpret 1.5 + jaq-parse 1.0) |
| 1.2-1.8 | Rewrite syn main.rs | **done** (478 lines, 43 tests) |
| 1.9 | Extract report_render_core | **done** (38 tests, shared with svalinn) |
| 1.10 | Orphan .qa cleanup in saga | **done** (removes stale sidecars before regeneration) |
| 2.1 | Deploy | **done** (saga + syn in deploy_tools.py) |
| 2.2 | E2E testing | **partial** (report mode tested, gate ratchet not yet) |
| 2.3 | Hook integration | **done** (hook_post_llm_tool wired, 10 tests) |
| 2.4 | PreToolUse gate | future (needs ratchet comparison engine) |

---

## Already Complete

- [x] saga_core library — pure types (SanityReport, Issue, path functions)
- [x] saga_runner library — generates .qa reports, directory walker, sidecar I/O
- [x] saga CLI — deployed to ~/.ai/tools/bin/saga
- [x] ~~qa_core library~~ — **DELETED**, code extracted to report_render_core
- [x] datagram — fire-and-forget Hlidskjalf broadcast
- [x] Hlidskjalf watchtower — receives and displays events
- [x] hook_io — all hooks emit to Hlidskjalf
- [x] deploy_tools.py — automated release build + symlink
- [x] format_core rebuild — JSON, YAML, TOML, TOON parse/serialize/convert (74 tests)
- [x] TOMLX parser — 7 files: types, annotation, units, paths, diagnostics, validation, processor (42 tests)
- [x] error_core — YamlParse, ToonParse, TomlxParse, Educational variants (10 tests)
- [x] jaq proof of concept — jaq-interpret 1.5 core-only handles all filter patterns
- [x] syn rewrite — 478 lines, report + gate modes, jq filtering, Hlidskjalf broadcast (43 tests)
- [x] report_render_core extraction — grouping, formatting, severity for QA consumers (38 tests)
- [x] qa_core/qa_report deleted, saga orphan cleanup added
- [x] process::exit refactored out of all helpers (15+ crates)
- [x] Hook shared code extracted to hook_io::rules
- [x] All tests passing
- [x] Full audit remediation (naming, organization, code quality)
