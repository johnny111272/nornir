# Nornir Improvement Plan

Generated 2026-03-20 from intersection of dual structural audits (A ∩ B) and dual documentation audits (A ∩ B).

**Previous plan (2026-03-19):** Items 1-4, 6-9, 11 completed. Items 5, 10 carried forward.

---

## Structural Findings (A ∩ B intersection)

### S1. Watcher transcript I/O should use session_io or write_engine

**Priority: Medium** (P1+P4 — pure logic in binary + composition violation)

`watch_and_diff_exchange_intercepts` reimplements transcript JSONL append via raw `OpenOptions`/`write_all` without fsync. The `session_io` crate handles session file I/O. The watcher should delegate transcript writing to session_io or use write_engine for fsync.

- Move `transcript_path_for()`, `open_transcript()`, `append_transcript()` to session_io
- Watcher imports from session_io instead of rolling its own
- Gains fsync discipline for crash safety

### S2. `split_jsonl_batches` `compute_batches()` extractable to core

**Priority: Low** (P1 — pure logic in binary)

Pure batch-sizing algorithm (takes total/min/max, returns Vec<usize>) with 10 tests. Single consumer today.

- Extract to appropriate core crate when a second consumer appears
- Not urgent — single consumer, well-tested in place

### S3. `hook_stop_llm_tts` has zero tests

**Priority: Medium** (P6 — untested hook binary)

123 lines, no test module. Contains testable logic: StopEvent deserialization, QUIET.lock checking, project directory resolution priority.

- Add tests for deserialization contract
- Add tests for lock file decision logic
- Add tests for project directory resolution

### S4. Delete orphaned `senders/announce/` directory

**Priority: Low** (P10 — orphaned artifact)

Untracked directory from the announce move to cli/. Should be deleted.

- `rm -rf senders/announce/`

### S5. `saga_runner` 545 lines, zero tests (A only — noted)

Significant capability crate with no unit tests. Tested indirectly via saga_cli and syn_cli.

---

## Documentation Findings (A ∩ B intersection)

### D1. CONTEXT_MAP.md: Refresh crate inventory

**Priority: High** (P1+P4 — stale counts cause reimplementation)

- Total workspace members: 95 → 97
- Core crates: 13 → 16 (missing: announce_core, time_core, text_core)
- Capability heading says 12, table has 11
- announce listed under senders (line 81) but lives in cli/ (line 116 says so — self-contradiction)
- "3 specialist tools" should be 4

Full regeneration needed. Carried forward from previous plan as item 5.

### D2. HOOK_DESIGN.md: SessionStart output contract wrong

**Priority: High** (P1 — wire format disagreement)

Doc says SessionStart output is `systemMessage`. Code produces `hookSpecificOutput.additionalContext` via `SessionStartResponse::to_json()`. A session building a hook would produce the wrong output.

- Fix event mapping table: SessionStart → `additionalContext`
- Also check PreCompact output contract (B found it also says `systemMessage` but code has no context field)

### D3. HOOK_DESIGN.md: Settings.json example incomplete

**Priority: Medium** (P1 — missing categories)

Example shows 3 of 6 bash hook categories. Missing: `--destruction`, `--revert`, `--workflow`.

- Update example to show all 6 categories with their configured severities

### D4. HOOK_DESIGN.md: Event type coverage incomplete

**Priority: Medium** (P3 — 7 documented, 11+ in code)

response.rs implements PostToolUseFailure, PermissionRequest, SubagentStop, SubagentStart, Notification, InstructionsLoaded, ConfigChange — none documented.

- Add missing event types to the event mapping table
- Mark any speculative/unused types as such

### D5. rules.rs: "Four tiers" comment but only three variants

**Priority: Low** (P1 — doc comment disagrees with code)

- Fix comment to say "Three tiers"

### D6. SYN_DESIGN.md: CLI reference missing flags (B only — noted)

`--stdin`, `--kind`, `--narrow` undocumented. The `--stdin` flag is mentioned in a code example but not in the CLI table.

### D7. NORNIR_CONVENTIONS.md: Dependency lookup table missing crates (A only — noted)

session_io, intercept_core, announce_core, time_core, text_core not in lookup table.

---

## Missing Design Documents (A ∩ B intersection)

### M1. Datagram/Hlidskjalf subsystem — no design doc

**Priority: Medium**

4 crates (datagram_core, datagram_io, record_datagrams, 5 send_* binaries) with dual transport, self-alerting, schema validation. No document explains why.

### M2. Gleipnir — no design doc

**Priority: Medium**

Largest core crate. AST analysis across 4 languages, check taxonomy, zone model, Svelte multi-phase. No design reference for sessions modifying gleipnir.

### M3. Write_engine — no design doc

**Priority: Medium**

Most-imported capability crate. WriterConfig, OutputPath, ai_home(), fsync discipline. No document explains the API.

### M4. Announce/voice subsystem — no design doc

**Priority: Medium**

announce_core + announce + hook_io speech integration. Voice directory convention, SILENT.lock, severity-to-voice mapping undocumented.

---

## Carried Forward

### C1. Refresh CONTEXT_MAP.md (was item 5)

Subsumes D1. Full regeneration from workspace Cargo.toml.

### C2. Add schema validation for announce configs (was item 10)

announce.toml, VOICE.lock, lookup tables, hits.toml — no schema files.

---

## Stale MEMORY.md entries (informational)

Audit B flagged:
- `io_filter crate orphaned` — already deleted
- `datagram_types naming exception` — already renamed to datagram_core

These entries should be cleaned from MEMORY.md.

---

## Execution Priority

1. **D1/C1** — Refresh CONTEXT_MAP.md (high damage, prevents reimplementation)
2. **D2** — Fix HOOK_DESIGN.md output contracts (high damage, wrong wire format)
3. **S3** — Add hook_stop_llm_tts tests (medium, untested binary)
4. **D3** — Fix HOOK_DESIGN.md settings example (medium, incomplete config)
5. **D4** — Add missing event types to HOOK_DESIGN.md (medium)
6. **S1** — Watcher transcript I/O extraction to session_io (medium)
7. **S4** — Delete senders/announce/ orphan (low, quick)
8. **D5** — Fix "Four tiers" comment (low, quick)
9. **M1-M4** — Design documents (medium, substantial writing)
10. **S2** — Extract compute_batches (low, defer until second consumer)
11. **C2** — Announce config schemas (low)

**Process:** Work items top-down. For each: plan mode → execute → verify → commit → next.
