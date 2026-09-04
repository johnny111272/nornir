# Documentation Audit A

**Auditor:** Claude Opus 4.6 (1M context) — Documentation Auditor A
**Date:** 2026-03-20
**Method:** For every actionable claim in workspace documentation, verified against actual implementation by reading code, checking file paths, checking function signatures, and comparing workspace Cargo.toml members.

---

## Priority 1: Disagreements Between Documentation and Implementation

### D1. CONTEXT_MAP: Core crate count and inventory

**Document says:** "13 core" crates, with a table listing 13 entries (error_core through intercept_core).

**Code says:** Workspace Cargo.toml has 16 core/ members. The core/ directory contains 16 crates. Three crates are missing from the CONTEXT_MAP inventory table: `announce_core`, `time_core`, `text_core`.

**Impact:** A fresh session will not know these crates exist and may reimplement their logic.

### D2. CONTEXT_MAP: Capability crate count

**Document says:** "12 capability" crates, with a table listing 12 entries including `session_io`.

**Code says:** Workspace Cargo.toml has 11 capability/ members. The capability/ directory contains 11 crates. The CONTEXT_MAP table lists `session_io` (which exists) but also lists `io_check` and `intercept_io`, which exist. Counting the table entries: `schemas_embedded`, `path_verify_io`, `io_check`, `gate_io`, `hook_io`, `datagram_io`, `intercept_io`, `write_engine`, `saga_runner`, `default_apply_io`, `session_io` = 11. The heading says 12. The heading count does not match the table.

### D3. CONTEXT_MAP: Total workspace member count

**Document says:** "95 workspace members."

**Code says:** Workspace Cargo.toml has 97 members.

**Breakdown of disagreement:** The doc's sub-totals (13+12+35+8+3+5+6+6+1+1+1+1+2+1=95) do not add up to the actual 97 (16+11+35+12+6+5+5+1+1+1+1+2+1=97). The core count is wrong (13 vs 16), capability count is wrong (12 vs 11), cli count is wrong (the doc says "8 checks + 3 tools" = 11 but cli/ has 12 members), and the senders list is wrong (see D4).

### D4. CONTEXT_MAP: `announce` listed as sender, actually in cli/

**Document says (line 81):** "6 senders in `senders/` -- `send_alert`, `send_warning`, `send_notification`, `send_heartbeat`, `send_datagram`, `announce`"

**Code says:** `announce` lives in `cli/announce/`. Workspace Cargo.toml lists it as `"cli/announce"`. The senders/ directory contains only 5 crates (the five `send_*` binaries). No `announce` directory exists in senders/.

**Internal contradiction:** The same document (line 116) says: "`announce` naming exception -- no verb prefix. Decomposed (item 1 done) and moved to cli/." Lines 81 and 116 of CONTEXT_MAP directly contradict each other about where `announce` lives.

### D5. CONTEXT_MAP: CLI tool count

**Document says:** "3 specialist tools in `cli/` -- `saga_cli` (binary: saga), `syn_cli` (binary: syn), `hush`"

**Code says:** cli/ contains 4 non-check crates: `saga_cli`, `syn_cli`, `hush`, and `announce`. The doc counts `announce` as a sender (D4) rather than as a cli tool, but it actually lives in cli/.

### D6. HOOK_DESIGN: SessionStart output contract says `systemMessage`

**Document says (line 29):** The SessionStart event mapping shows output contract as `systemMessage`.

**Code says:** `response.rs` `SessionStartResponse::to_json()` (lines 636-650) produces `hookSpecificOutput.additionalContext`, not top-level `systemMessage`. The `systemMessage` field is available via the `WithUniversal` trait as a separate top-level field, but it is not the default output. A session following the doc's event mapping table would misunderstand which JSON field carries the session start context.

### D7. HOOK_DESIGN: Settings.json example missing `--workflow` flag

**Document says (line 246):** `hook_pre_llm_bash --subversion block --truncation warn --evasion warn`

**Likely state:** Session memory records that `--workflow ask` was added to the hook_pre_llm_bash command in settings.json. The HOOK_DESIGN example may be stale. This requires human verification against the actual `~/.claude/settings.json` (outside the repo).

### D8. hook_post_llm_tool doc comment says `systemMessage`, code produces `additionalContext`

**Document says:** The file-level doc comment in `hooks/hook_post_llm_tool/src/main.rs` line 6 says: "injects the TOON assessment as a systemMessage."

**Code says:** The function calls `hook_io::run_post_hook(assess)`, which produces `hookSpecificOutput.additionalContext` (via `PostToolUseResponse`). The HOOK_DESIGN document (line 293) correctly says `hookSpecificOutput.additionalContext`. The binary's own doc comment disagrees with both the design doc and the code behavior.

### D9. STRUCTURAL_AUDIT_GUIDE: Gleipnir Rust check list incomplete

**Document says (Priority 9, lines 182-191):** Lists Rust checks as: `no_unwrap`, `no_println`, `no_clone_spam`, `no_string_abuse`, `no_pub_overuse`, `no_underscore_prefix`, `function_length_rs`, `nesting_depth_rs`, `short_names` / `numbered_names`, `suppression`.

**Code says:** `gleipnir_core/src/lib.rs` lines 121-138 shows 13 Rust checks. The audit guide is missing: `param_count_rs` and `short_param_names_rs`. Also the guide names are slightly imprecise: `short_names` should be `no_single_letter_names_rs`, `numbered_names` should be `no_numbered_suffixes_rs`, `suppression` should be `no_suppression_comments_rs`.

### D10. STRUCTURAL_AUDIT_GUIDE: Gleipnir Python check list incomplete

**Document says (lines 227-232):** Lists Python checks including "Import violations (cross-zone, relative, parent), type safety (Any, bare dict/list, large unions), Architecture (classes outside structures/, god classes, dataclass usage)."

**Code says:** The actual Python check matrix includes additional checks not mentioned in the audit guide summary: `import_count`, `no_reexport_shims`, `check_no_overload`, `check_no_model_dump`, `check_no_dunder_all`, `check_init_files_empty`, `check_no_future_annotations`, `impure_module_quarantine`, `check_structures_import_boundary`, `check_structures_no_functions`, `check_max_functions_outside_zones`, `check_no_throwaway_assignment`, `check_no_json_value`, `check_no_implicit_type_aliases`, `check_hardcoded_config`, `check_no_cast`. The list in the audit guide is a high-level summary, not exhaustive, but may mislead sessions into thinking unlisted checks don't exist.

### D11. rules.rs doc comment says "Four tiers" but lists three

**Document says:** `rules.rs` line 13: "Four tiers (ascending enforcement):" then lists three: Warn, Ask, Block.

**Code says:** The `Severity` enum has exactly three variants: `Warn`, `Ask`, `Block`.

---

## Priority 2: Missing Design Rationale

### M1. No design document for the datagram/Hlidskjalf subsystem

Four crates form this subsystem: `datagram_core` (types), `datagram_io` (transport), `send_*` binaries (5 senders), `record_datagrams` (daemon). No committed document explains:
- Why dual transport (Unix socket + UDP multicast)?
- What is the datagram schema contract?
- What is `record_datagrams` and how does it relate to Hlidskjalf?
- When should a new sender be created vs using `send_datagram` directly?

A fresh session encountering this subsystem would not understand the architecture and might add transport logic to a sender binary or hardcode the socket path.

### M2. No design document for the quality pipeline (saga_runner + saga_core + report_render_core)

SYN_DESIGN.md explains syn. HOOK_DESIGN.md explains the hook integration. But there is no design document for saga itself. Three crates form the truth-recording layer: `saga_core` (types), `saga_runner` (generation + I/O), `report_render_core` (formatting). No committed document explains:
- What quality tools saga orchestrates and how
- What the `.qa` sidecar format is
- Why report_render_core was extracted from saga_runner
- What Svalinn is and how it consumes `.qa` files

### M3. No design document for gleipnir_core

`gleipnir_core` is the largest core crate (multiple check modules across 4 languages, a classification matrix, parsing). No design document explains:
- The FileKind classification system and zone model
- Why checks are organized by check-category (prohibited, style, suppression, architecture) within language modules
- The CheckConfig system and how it interacts with the matrix
- How the Svelte multi-phase check pipeline works (script/style/template/raw)

The STRUCTURAL_AUDIT_GUIDE references gleipnir checks but only as "what not to audit." A session modifying gleipnir has no design reference.

### M4. No design document for the write_engine capability crate

`write_engine` is foundational -- nearly every binary depends on it. It provides `ai_home()`, `run()`, `append_line_fsync()`, `write_truncate_fsync()`. No document explains:
- The WriterConfig declarative pattern and why it exists
- What `OutputPath::DirectoryName` vs other variants do
- Why `ai_home()` lives in write_engine rather than a separate crate
- The fsync discipline and why it matters

### M5. No design document for announce_core + announce

`announce_core` and `announce` (cli/) form a voice alert subsystem. `hook_io` calls `announce` as a subprocess for speech alerts. No document explains the architecture, severity-to-voice mapping, or the relationship with SILENT.lock and the voice directory.

---

## Priority 3: Contradictions Between Documents

### C1. CONTEXT_MAP self-contradiction on `announce` location

CONTEXT_MAP line 81 lists `announce` under "6 senders in `senders/`". CONTEXT_MAP line 116 says `announce` was "moved to cli/". Same document, two conflicting statements. The code agrees with line 116.

### C2. HOOK_DESIGN vs hook_post_llm_tool doc comment on output field

HOOK_DESIGN line 293 says the PostToolUse assessment is returned as `hookSpecificOutput.additionalContext`. The `hook_post_llm_tool/src/main.rs` doc comment (line 6) says the assessment is injected as a `systemMessage`. The code behavior matches HOOK_DESIGN. The binary's own doc comment is wrong relative to both the design doc and the implementation.

---

## Priority 4: Stale Inventories

### S1. Three core crates invisible in CONTEXT_MAP

The following crates exist in workspace Cargo.toml and on disk but are absent from the CONTEXT_MAP Tier 1 inventory table:

| Crate | On disk | In workspace Cargo.toml | In CONTEXT_MAP |
|-------|---------|------------------------|----------------|
| `announce_core` | `core/announce_core/` | Yes | No |
| `time_core` | `core/time_core/` | Yes | No |
| `text_core` | `core/text_core/` | Yes | No |

A future session will not know these crates exist. `announce_core` is particularly important as `announce` (cli/) depends on it and it presumably contains the voice/speech logic that should not be reimplemented.

### S2. NORNIR_CONVENTIONS Dependency Lookup table missing crates

The Dependency Lookup table in NORNIR_CONVENTIONS.md lists what to use for various needs. The following capability crates have no entry: `default_apply_io`, `intercept_io`, `session_io`. The following core crates have no entry: `announce_core`, `time_core`, `text_core`, `compaction_inject_core`, `intercept_core`, `default_apply_core`.

While not all crates need lookup entries (some are niche), `session_io` and `intercept_core` are important enough that sessions working on the intercept pipeline should find them in the lookup table.

### S3. deploy_categories.toml: `announce` is in `[tools]` category

`deploy_categories.toml` lists `announce` under `[tools]` alongside `saga_cli`, `syn_cli`, `hush`. CONTEXT_MAP does not reflect this -- it calls `announce` a sender. This means `nornir_deploy --build senders` will NOT build `announce`. A session following CONTEXT_MAP's categorization would deploy `announce` wrong.

### S4. intercept_replay cc_wire_schema.json: symlink confirmed

INTERCEPT_DESIGN.md says the replay schema is a symlink. Verified: `/Users/johnny/ai/smidja/nornir/interceptors/intercept_replay/cc_wire_schema.json` is a symlink pointing to `../traffic_interceptor_rewriter/cc_wire_schema.json`. No disagreement -- this is confirmed correct.

---

## Summary of Findings

| Priority | Count | Key items |
|----------|-------|-----------|
| P1: Disagreements | 11 | Crate counts (D1-D5), output contracts (D6, D8), settings example (D7), check lists (D9-D10), enum count (D11) |
| P2: Missing design docs | 5 | Datagram/Hlidskjalf (M1), Saga pipeline (M2), Gleipnir (M3), write_engine (M4), announce (M5) |
| P3: Contradictions | 2 | announce location (C1), systemMessage vs additionalContext (C2) |
| P4: Stale inventories | 3 | Invisible core crates (S1), missing lookup entries (S2), deploy category mismatch (S3) |

### Highest-damage items for immediate attention

1. **D1 + S1: Three invisible core crates.** `announce_core`, `time_core`, `text_core` exist but are undocumented. A session will reimplement their logic.
2. **D4 + C1 + S3: `announce` location confusion.** Three documents disagree about where announce lives and how it deploys. A session will deploy it wrong.
3. **D6 + D8 + C2: `systemMessage` vs `additionalContext` confusion.** Two documents say `systemMessage`, the code produces `additionalContext`. A session building a new hook will use the wrong output field.
4. **M1: No datagram subsystem design doc.** Five senders + a daemon + dual transport with no explanation. Sessions will misuse the subsystem.
