# Documentation Audit A

**Auditor:** Documentation Auditor A
**Date:** 2026-03-19
**Method:** Verified documentation claims against implementation by reading source code, checking file paths, comparing function signatures, enum variants, CLI flags, and crate inventories against workspace Cargo.toml.

---

## Priority 1: Disagreements Between Documentation and Implementation

### D1. HOOK_DESIGN.md — Severity enum (line 135)

**Doc says:** `pub enum Severity { Warn, Block }`
**Code says:** `pub enum Severity { Warn, Ask, Block }` (capability/hook_io/src/rules.rs line 17)

The `Ask` variant exists in the implementation but is absent from the HOOK_DESIGN.md Severity enum listing. The doc comment in rules.rs describes "Four tiers" but only lists three variants -- that internal doc comment is also inconsistent (says four, shows three). The CONTEXT_MAP.md already flags this as known stale.

### D2. HOOK_DESIGN.md — RawRule struct (line 138)

**Doc says:** `pub struct RawRule { pub pattern: String, pub description: String }`
**Code says:** `pub struct RawRule { pub pattern: String, pub description: String, pub severity: Option<Severity> }` (capability/hook_io/src/rules.rs line 41-45)

The per-rule severity override field is missing from the HOOK_DESIGN.md struct definition.

### D3. HOOK_DESIGN.md — PostToolUse output contract (lines 92-97)

**Doc says:** PostToolUse output is `{"systemMessage": "..."}`
**Code says:** PostToolUseResponse puts context into `{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "..."}}` (capability/hook_io/src/response.rs lines 346-367). A top-level `systemMessage` is available via `Universal` fields but is NOT what `run_post_hook` uses -- `run_post_hook` calls `PostToolUseResponse::allow().with_context(msg)` which populates `hookSpecificOutput.additionalContext`.

Disagreement: the wire format shown in the doc does not match the wire format produced by the code. Which is the intended contract?

### D4. HOOK_DESIGN.md — PostToolUse dispatch table (lines 182-189)

**Doc says:** `.py --> assess_python(file_path)`, `.rs --> None (future: cargo check)`, `.toml -> None`, `_ --> None`
**Code says:** The function is named `assess_source` (not `assess_python`). It handles `.py`, `.rs`, AND `.svelte` (all routed to the same saga/syn pipeline). There is no Python-specific runner -- all three extensions use the identical pipeline. (hooks/hook_post_llm_tool/src/main.rs lines 26-28, 60)

Disagreements:
- Function name: `assess_python` vs actual `assess_source`
- Rust files: doc says "None (future)" but code routes `.rs` to the saga/syn pipeline (same as Python)
- Svelte files: not mentioned in the doc at all, but handled in the code
- The doc implies per-language runners, but the actual implementation is a single unified `assess_source` for all supported extensions

### D5. HOOK_DESIGN.md — `deploy_hooks.py` reference (line 222)

**Doc says:** `deploy_hooks.py builds release binaries and symlinks into ~/.ai/tools/bin/`
**Reality:** `deploy_hooks.py` does not exist. Deployment is handled by `nornir_deploy --build hooks`. The CONTEXT_MAP.md already flags this.

### D6. HOOK_DESIGN.md — Workspace members list (lines 213-218)

**Doc says:** Five hooks listed: `hook_pre_llm_tool`, `hook_pre_llm_bash`, `hook_pre_subagent_tool`, `hook_pre_subagent_bash`, `hook_post_llm_tool`
**Code says:** Six hooks exist in the workspace: the five listed plus `hook_stop_llm_tts` (hooks/ directory and workspace Cargo.toml both confirm).

The `hook_stop_llm_tts` hook is missing from HOOK_DESIGN.md's workspace members listing.

### D7. SYN_DESIGN.md — PostToolUse integration (lines 289-294)

**Doc says:** "Syn gate mode: compares fresh .qa against baseline sidecar" and "Decision returned as additionalContext (TOON format)"
**Code says:** `hook_post_llm_tool` runs syn with only `--stdin` (no `--mode gate`). Since `--mode report` is the default (cli/syn_cli/src/main.rs line 129), the hook uses **report mode**, not gate mode.

The CONTEXT_MAP.md already flags this ("PostToolUse section claims gate mode but actual is report mode").

### D8. NORNIR_CONVENTIONS.md — Stale deploy script references

**Doc says (line 128):** "add to workspace Cargo.toml and `deploy_writers.py`"
**Doc says (line 216):** "Gate crates (PyO3) fail to link without Python headers -- use `deploy_gates.py` for those."
**Reality:** Neither `deploy_writers.py` nor `deploy_gates.py` exist. The correct tool is `nornir_deploy --build writers` and `nornir_deploy --build gates`. The CONTEXT_MAP.md already flags this.

### D9. NORNIR_CONVENTIONS.md — Test exclusion command (line 217)

**Doc says:** `cargo test --workspace $(for d in gates/*/; do echo "--exclude $(basename $d)"; done) --exclude intercept_io`
**Code says:** `traffic_interceptor_rewriter` is also a PyO3 crate (cdylib+rlib with pyo3 dependency in its Cargo.toml) that would fail to link without Python headers. The doc's exclusion list omits `--exclude traffic_interceptor_rewriter`.

The MEMORY.md has the correct command with both `--exclude intercept_io --exclude traffic_interceptor_rewriter`, but the committed NORNIR_CONVENTIONS.md does not.

### D10. CONTEXT_MAP.md — Workspace member count (line 36)

**Doc says:** "98 workspace members: 13 core + 12 capability + 35 gates + 8 checks + 3 tools + 5 writers + 6 hooks + 6 senders + 1 rewriter + 1 converter + 1 dispatcher + 1 watcher + 2 interceptors + 1 daemon"
**Code says:** 95 workspace members (counted from Cargo.toml). The breakdown: 13 core + 12 capability + 35 gates + 8 checks + 3 tools (cli) + 5 writers + 6 hooks + 6 senders + 1 rewriter + 1 converter + 1 dispatcher + 1 watcher + 2 interceptors + 1 daemon = 96 if you count hush (in cli/) separately from checks. But the workspace Cargo.toml has exactly 95 members.

The sum 13+12+35+8+3+5+6+6+1+1+1+1+2+1 = 96, which disagrees with both the stated "98" and the actual 95. The count math itself is internally inconsistent.

### D11. CONTEXT_MAP.md — Binary count (line 73)

**Doc says:** "67 executables + 35 gate modules"
**Code says:** Non-gate binary crates: 8 checks + 3 tools + 5 writers + 6 hooks + 6 senders + 1 rewriter + 1 converter + 1 dispatcher + 1 watcher + 1 intercept_replay + 1 daemon = 34 executables (traffic_interceptor_rewriter is a PyO3 lib, not an executable). 34 + 35 = 69 total, not 67 + 35. The "67 executables" count disagrees with actual.

### D12. NORNIR_CONVENTIONS.md — Workspace dependencies list (line 239)

**Doc says:** Lists `serde, serde_json, serde_yaml, toml, toon-format, jsonschema, thiserror, pyo3, regex, jaq-interpret, jaq-parse, tree-sitter, tree-sitter-python, tree-sitter-rust, tree-sitter-typescript, sha2, clap, libc`
**Code says:** Workspace Cargo.toml also includes: `tree-sitter-css 0.25`, `tree-sitter-html 0.23`, `reqwest 0.12`, `dotenvy 0.15`, `rodio 0.19`. These five workspace dependencies are missing from the documentation.

### D13. STRUCTURAL_AUDIT_GUIDE.md — Gleipnir check categories (lines 181-191)

**Doc says:** Lists Rust checks: `no_unwrap`, `no_println`, `no_clone_spam`, `no_string_abuse`, `no_pub_overuse`, `no_underscore_prefix`, `function_length_rs`, `nesting_depth_rs`, `short_names` / `numbered_names`, `suppression`
**Code says:** The gleipnir matrix (core/gleipnir_core/src/matrix.rs) also registers `import_count` for Python checks (lines 224, 268). Additionally, gleipnir has a `checks_svelte` module (checks for Svelte files) that is not mentioned in the audit guide's "What Gleipnir Covers" section at all.

The doc claims TypeScript checks exist but doesn't mention Svelte. The code has `checks_svelte/` as a separate module.

### D14. HOOK_DESIGN.md — Event mapping table and hook_stop_llm_tts

**Doc says (line 40):** `hook_stop_llm_tts` is listed as event "Stop" with context "LLM" and scope "TTS playback"
**Doc says (lines 24-28):** The event mapping table lists: PreToolUse, PostToolUse, SessionStart, SessionEnd, PreCompact. There is no "Stop" event in the mapping table.

The `hook_stop_llm_tts` hook uses an event type that is not defined in the HOOK_DESIGN.md event mapping table.

### D15. HOOK_DESIGN.md — response.rs supports many more event types than documented

**Doc says:** Event mapping covers PreToolUse, PostToolUse, SessionStart, SessionEnd, PreCompact.
**Code says:** response.rs implements builders for: PreToolUse, PermissionRequest, PostToolUse, PostToolUseFailure, UserPromptSubmit, Stop, SubagentStop, ConfigChange, SessionStart, SubagentStart, Notification, PreCompact, SessionEnd, InstructionsLoaded. That is 14 event types vs 5 documented.

The response module has grown far beyond what HOOK_DESIGN.md describes. Whether the doc should be updated or the extra response types are speculative implementations is a human judgment call.

---

## Priority 2: Missing Design Rationale

### M1. Intercept subsystem has no committed design document

Four crates form the intercept pipeline: `intercept_core` (classification), `session_io` (shared file I/O), `traffic_interceptor_rewriter` (live PyO3 module), `intercept_replay` (reconstruction binary). Additional crates participate: `compaction_inject_core`, `diff_core`, `watch_and_diff_exchange_intercepts`.

There is no committed document explaining:
- Why `session_io` was extracted from the interceptor
- What `record_compaction` does vs `inject_compaction_system_block` (record_compaction handles the shared capture/truncate/datagram flow; inject_compaction_system_block modifies the request body -- live only)
- Why replay skips injection but uses the same classification
- The relationship between the watcher and the replay binary
- The raw_session_log.jsonl -> derived files reconstruction flow
- The compaction capture/truncate/inject sequence

The design rationale lives only in MEMORY.md (session memory). A fresh session with no memory would not understand why these crates are structured this way and would likely add functions directly to `traffic_interceptor_rewriter`.

### M2. Datagram subsystem has no design document

Three crates form the datagram subsystem: `datagram_types` (core types), `datagram_io` (emission), and `record_datagrams` (daemon). Six sender binaries consume it. The watcher and hooks also emit datagrams.

No document explains:
- The dual-transport architecture (Unix socket + what else?)
- Why `datagram_types` exists separately from `datagram_io`
- The socket path ownership rule (only in `datagram_io`)
- What `record_datagrams` daemon does with received datagrams
- The relationship between datagrams and the Hlidskjalf dashboard

### M3. The `announce` binary has no design document

At 658 lines, `announce` is the largest binary in the workspace. It is a full ElevenLabs TTS application with caching, profile management, language lookup, and audio playback. It lives in `senders/` but is architecturally a standalone CLI application, not a thin datagram sender.

No document explains:
- Why it is in `senders/` rather than `cli/`
- The caching architecture (hash-based filename, voice directory structure)
- The profile/config system (announce.toml, VOICE.lock)
- The language lookup system
- Why it has zero internal nornir dependencies (no write_engine, no datagram_io, no ai_home)

The IMPROVEMENT_PLAN.md item 1 calls for decomposition, but no design document captures the current architecture or the intended target architecture.

---

## Priority 3: Contradictions Between Documents

### C1. SYN_DESIGN.md vs itself: PostToolUse mode

Line 289-293 says the PostToolUse hook uses "Syn gate mode" for per-file ratchet comparison.
Line 329-330 says "Gate mode: filtering works, decision logic works, **ratchet comparison NOT YET IMPLEMENTED**"

If gate mode's core feature (ratchet) is not implemented, the PostToolUse integration section's claim that it uses gate mode is either aspirational or wrong. The actual code uses report mode.

### C2. HOOK_DESIGN.md "additionalContext" vs "systemMessage"

Line 25 says PreToolUse output includes `additionalContext`.
Line 92-97 says PostToolUse output is `{"systemMessage": "..."}`.
Line 452 (run_post_hook docstring) says "Some(msg) injects into LLM context via additionalContext."

The PostToolUse section claims `systemMessage`, the docstring claims `additionalContext`, and the code produces `hookSpecificOutput.additionalContext`. Three descriptions, two different field names.

---

## Priority 4: Stale Inventories

### S1. CONTEXT_MAP.md — `hush` not categorized in inventory

`hush` is listed under "3 specialist tools in cli/" alongside saga_cli and syn_cli. But hush is described in the Current hooks table (HOOK_DESIGN.md line 41) as a UserPromptSubmit hook, and CONTEXT_MAP.md's known issues section calls it a "UserPromptSubmit hook" that is misplaced. The inventory categorizes it as a tool while acknowledging it is a hook -- this sends mixed signals to a session trying to understand the architecture.

### S2. CONTEXT_MAP.md does not list `announce` in the senders inventory description

The Tier 3 binaries section (line 80) lists "6 senders" with names: `send_alert`, `send_warning`, `send_notification`, `send_heartbeat`, `send_datagram`, `announce`. The `announce` binary IS listed, but the description says "6 senders in senders/" while the known issues section (line 116) notes it is architecturally misplaced. This is correctly flagged already.

### S3. deploy_categories.toml vs CONTEXT_MAP.md

deploy_categories.toml lists `intercept_replay` under `[interceptors]` as a cargo crate and `traffic_interceptor_rewriter` as a pyo3_crate. CONTEXT_MAP.md (line 85) correctly describes this: "2 interceptors -- traffic_interceptor_rewriter (PyO3 module), intercept_replay (binary)". No disagreement here -- this is consistent.

---

## Summary of Findings

**Priority 1 (Disagreements):** 15 found
- 6 in HOOK_DESIGN.md (Severity enum, RawRule struct, PostToolUse wire format, dispatch table, deploy script, workspace members)
- 3 in NORNIR_CONVENTIONS.md (deploy script refs x2, test exclusion command)
- 3 in CONTEXT_MAP.md (member count, binary count, workspace deps)
- 2 in SYN_DESIGN.md (PostToolUse mode)
- 1 in STRUCTURAL_AUDIT_GUIDE.md (gleipnir check list incomplete)

**Priority 2 (Missing design docs):** 3 found
- Intercept subsystem (4 crates, no design doc)
- Datagram subsystem (3 crates, no design doc)
- Announce binary (658 lines, no design doc)

**Priority 3 (Contradictions):** 2 found
- SYN_DESIGN.md contradicts itself on PostToolUse mode
- HOOK_DESIGN.md contradicts itself on PostToolUse wire format field name

**Most damaging to future sessions:** M1 (missing intercept design doc) and D3/D4 (HOOK_DESIGN.md PostToolUse section). A session working on the intercept pipeline with no design doc will misplace code. A session reading HOOK_DESIGN.md's PostToolUse section will write code against the wrong wire format and the wrong dispatch architecture.
