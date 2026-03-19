# Documentation Audit B — Strict Verification Against Implementation

**Auditor:** Documentation Auditor B
**Date:** 2026-03-19
**Method:** Every actionable claim in every committed document verified against actual source code, Cargo.toml, and filesystem state.

---

## Priority 1: Disagreements Between Documentation and Implementation

### D1. HOOK_DESIGN.md — Severity enum has two variants, code has three

**Document says (line 135):**
```rust
pub enum Severity { Warn, Block }
```

**Code says (`capability/hook_io/src/rules.rs` line 16-21):**
```rust
pub enum Severity {
    Warn,
    Ask,
    Block,
}
```

The `Ask` variant is missing from the document. The `parse_severity` function also handles `"ask"` (line 29). CONTEXT_MAP flags this but the document itself has not been corrected.

---

### D2. HOOK_DESIGN.md — HookDecision enum has three variants, code has four

**Document says (line 89):**
> `HookDecision` is `Allow | Warn { ... } | Deny { ... }`

**Code says (`capability/hook_io/src/lib.rs` lines 49-73):**
`HookDecision` has four variants: `Allow`, `Warn { category, event, user_reason, llm_context }`, `Ask { category, event, reason, llm_context }`, `Deny { category, event, reason }`.

The `Ask` variant is undocumented. Its fields also differ from `Warn` (uses `reason` not `user_reason`).

---

### D3. HOOK_DESIGN.md — PostToolUse output format is wrong

**Document says (lines 92-97):**
```json
{
    "systemMessage": "..."
}
```

**Code says (`capability/hook_io/src/response.rs` `PostToolUseResponse::to_json()`, lines 346-367):**
PostToolUse output is:
```json
{
    "hookSpecificOutput": {
        "hookEventName": "PostToolUse",
        "additionalContext": "..."
    }
}
```

The document claims the output key is `systemMessage` at the top level. The code puts it in `hookSpecificOutput.additionalContext`. This is a wire format disagreement that would cause a future session to build a PostToolUse hook with the wrong output shape.

---

### D4. HOOK_DESIGN.md — PostToolUse dispatch table references wrong function name

**Document says (line 182):**
```
+-- .py  --> assess_python(file_path)
```

**Code says (`hooks/hook_post_llm_tool/src/main.rs` line 60):**
The function is `assess_source(file_path)`, not `assess_python`. It handles `.py`, `.rs`, and `.svelte` files — the document only mentions `.py`.

The dispatch table also says `.rs --> None (future: cargo check)` but the actual code routes `.rs` to `assess_source`, which runs the `saga | syn` pipeline on Rust files.

---

### D5. HOOK_DESIGN.md — Workspace members section incomplete

**Document says (lines 212-218):**
Lists 5 hooks: `hook_pre_llm_tool`, `hook_pre_llm_bash`, `hook_pre_subagent_tool`, `hook_pre_subagent_bash`, `hook_post_llm_tool`.

**Actual workspace Cargo.toml:**
6 hooks: the above 5 plus `hook_stop_llm_tts`.

The hook is listed in the "Current hooks" table earlier in the document but omitted from the workspace members section.

---

### D6. HOOK_DESIGN.md — Deploy references deleted script

**Document says (line 222):**
> `deploy_hooks.py` builds release binaries and symlinks into `~/.ai/tools/bin/`.

**Actual:** `deploy_hooks.py` does not exist. Deploy is now handled by `nornir_deploy --build hooks`. CONTEXT_MAP flags this but the document has not been corrected.

---

### D7. SYN_DESIGN.md — PostToolUse integration section claims gate mode

**Document says (lines 289-296):**
> After every Python file write:
> 1. Saga runs on the written file -> produces fresh .qa
> 2. Syn gate mode: compares fresh .qa against baseline sidecar

**Code says (`hooks/hook_post_llm_tool/src/main.rs` lines 72-73):**
The hook runs `syn --stdin` which uses report mode (the default). No `--mode gate` flag is passed. Syn is used in report mode, not gate mode. The document describes behavior that is not implemented.

Additionally, the document says "After every Python file write" but the code also handles `.rs` and `.svelte` files.

---

### D8. NORNIR_CONVENTIONS.md — References deleted deploy scripts

**Document says (line 128):**
> To add a new writer: create schema in `schemas/tools/`, add to `schemas_embedded/src/lib.rs`, create writer crate, add to workspace `Cargo.toml` and `deploy_writers.py`.

**Document says (line 216):**
> Gate crates (PyO3) fail to link without Python headers -- use `deploy_gates.py` for those.

**Actual:** Neither `deploy_writers.py` nor `deploy_gates.py` exist. Deploy is handled by `nornir_deploy --build writers` and `nornir_deploy --build gates` respectively.

---

### D9. NORNIR_CONVENTIONS.md — Workspace dependencies list incomplete

**Document says (line 239):**
Lists workspace dependencies ending with `libc 0.2`.

**Actual workspace Cargo.toml (lines 114-124):**
Five additional dependencies are present but unlisted in the document:
- `tree-sitter-css = "0.25"`
- `tree-sitter-html = "0.23"`
- `reqwest = { version = "0.12", features = ["blocking"] }`
- `dotenvy = "0.15"`
- `rodio = "0.19"`

---

### D10. NORNIR_CONVENTIONS.md — Test exclusion command incomplete

**Document says (line 217):**
```bash
cargo test --workspace $(for d in gates/*/; do echo "--exclude $(basename $d)"; done) --exclude intercept_io
```

**Actual:** `traffic_interceptor_rewriter` is also a PyO3 crate (`cdylib+rlib`) that will fail to link without Python headers. The documented command will fail when it reaches this crate. The correct exclusion also requires `--exclude traffic_interceptor_rewriter`.

---

### D11. CONTEXT_MAP.md — Workspace member count is wrong

**Document says (line 36):**
> 98 workspace members: 13 core + 12 capability + 35 gates + 8 checks + 3 tools + 5 writers + 6 hooks + 6 senders + 1 rewriter + 1 converter + 1 dispatcher + 1 watcher + 2 interceptors + 1 daemon.

**Actual:** The workspace has 95 members (counted from Cargo.toml). The itemized list sums to 96 (13+12+35+8+3+5+6+6+1+1+1+1+2+1), not 98. Neither 96 nor 98 equals the actual 95. One category count is off; the most likely discrepancy is that CONTEXT_MAP counts some crate(s) in multiple categories, or a crate was removed after the document was generated.

Verified per-category from Cargo.toml: core=13, capability=12, gates=35, cli=11 (8 checks + 3 tools), hooks=6, senders=6, writers=5, interceptors=2, watchers=1, rewriters=1, dispatchers=1, daemons=1, converters=1. Total = 95.

---

### D12. HOOK_DESIGN.md — Settings.json wiring omits subagent hooks

**Document says (lines 227-245):**
Shows settings.json with only PreToolUse matchers for LLM (tool + bash) and PostToolUse for LLM. No configuration shown for `hook_pre_subagent_tool` or `hook_pre_subagent_bash`.

These hooks exist in the workspace and in `deploy_categories.toml` but the document's settings.json example does not show how they are wired. A future session adding subagent hooks would not know the intended wiring pattern.

---

### D13. NORNIR_CONVENTIONS.md — Writer composition pattern says "add to deploy_writers.py"

**Document says (line 128):**
> To add a new writer: ... add to workspace `Cargo.toml` and `deploy_writers.py`.

The same section should reference `deploy_categories.toml` instead of the deleted `deploy_writers.py`.

---

## Priority 2: Missing Design Rationale

### M1. Intercept/session subsystem has no design document

The intercept pipeline spans 5 crates: `intercept_core`, `session_io`, `traffic_interceptor_rewriter`, `intercept_replay`, `compaction_inject_core`. Plus it interacts with `watch_and_diff_exchange_intercepts` and `diff_core`.

There is no committed document explaining:
- Why `session_io` was extracted from the interceptor
- What `record_compaction` does vs `inject_compaction_system_block` (session_io does shared compaction; the interceptor additionally injects and rewrites; replay calls only session_io)
- Why replay skips `inject_compaction_system_block`
- The relationship between `append_raw` (live only, in interceptor) and session_io (shared)
- The file layout within `sessions/{session_id}/`
- How the watcher subprocess is spawned by replay

This design knowledge currently exists only in session memory. The next session working on the intercept pipeline will see 5+ crates and not understand the extraction boundaries.

---

### M2. Datagram subsystem has no design document

Three crates form the datagram pipeline: `datagram_types`, `datagram_io`, `record_datagrams`. Plus, 6 sender binaries use `datagram_io`.

There is no committed document explaining:
- The dual-transport architecture (Unix stream socket + validated JSON)
- Why `datagram_types` is in core/ and `datagram_io` is in capability/
- The relationship between `emit()` and `emit_validated_or_alert()`
- The `record_datagrams` daemon's daily rotation scheme
- The Hlidskjalf integration contract (what listens on the socket)

---

### M3. Write engine / writer pattern has no design document

`write_engine` is a capability crate that 5 writer binaries depend on. It provides `run()`, `append_line_fsync()`, `write_truncate_fsync()`, and `ai_home()`. The conventions document describes the writer binary pattern but does not explain:
- The `WriterConfig` contract and its fields (`OutputFormat`, `OutputPath`, `WriteFrequency`)
- What "atomic writes" means in this context (rename? fsync?)
- Why `ai_home()` exists in write_engine rather than in a more general utility crate
- How `append_line_fsync` differs from standard append

---

### M4. Gleipnir subsystem has no design document

`gleipnir_core` is a core crate containing the tree-sitter AST guardrail engine. It is referenced by `saga_runner` and drives the quality pipeline. The STRUCTURAL_AUDIT_GUIDE describes what gleipnir checks but there is no design document explaining:
- The tree-sitter query architecture
- How checks are registered and configured
- The check categories and their intended scope (Rust, Python, TypeScript, Svelte)
- How exemption rules work internally
- The relationship between gleipnir checks and `.gleipnir/` config directories

---

## Priority 3: Contradictions Between Documents

### C1. SYN_DESIGN.md contradicts itself on PostToolUse mode

**Line 289-293 (Integration Points > PostToolUse Hook):**
> Syn gate mode: compares fresh .qa against baseline sidecar

**Lines 329-330 (Current State):**
> Gate mode: filtering works, decision logic works, **ratchet comparison NOT YET IMPLEMENTED**

If ratchet comparison is not implemented, the PostToolUse hook cannot be using gate mode as described in lines 289-293. The document contradicts itself within 40 lines.

---

### C2. HOOK_DESIGN.md and response.rs disagree on PostToolUse output field name

- HOOK_DESIGN.md line 95-97: `"systemMessage": "..."`
- response.rs `PostToolUseResponse::to_json()`: `hookSpecificOutput.additionalContext`
- hook_io `run_post_hook` doc comment (line 451): "injects into LLM context via additionalContext"

Two of three sources say `additionalContext`. One says `systemMessage`. The hook_io doc comment and the code agree; the design document disagrees.

---

### C3. CONTEXT_MAP and CLAUDE.md name the structural audit guide differently

- CONTEXT_MAP line 15: `audit/STRUCTURAL_AUDIT_GUIDE.md`
- CONTEXT_MAP line 27: `audit/STRUCTURAL_AUDIT_GUIDE.md`
- CLAUDE.md line 65: `audit/STRUCTURAL_AUDIT_GUIDE.md` (referenced as `audit/AUDIT_GUIDE.md` in some formulations)

The actual file on disk is `audit/STRUCTURAL_AUDIT_GUIDE.md`. These are consistent. No contradiction found on re-verification.

---

## Priority 4: Stale Inventories

### S1. CONTEXT_MAP intercept_core description understates exports

**Document says:**
> Exchange classification: ExchangeKind, classify_exchange, has_tool

**Code also exports:**
- `tool_count` (public function, used by `classify_exchange` and available to consumers)

Minor, but a future session looking for tool counting logic would not know it exists in `intercept_core`.

---

### S2. CONTEXT_MAP does not mention response module event types

The `hook_io` capability crate's `response` module contains response builders for 11 event types: PreToolUse, PostToolUse, PostToolUseFailure, UserPromptSubmit, Stop, SubagentStop, ConfigChange, SessionStart, SubagentStart, Notification, PreCompact, SessionEnd, InstructionsLoaded.

HOOK_DESIGN.md only documents PreToolUse and PostToolUse output contracts. The response module supports far more event types than any document describes. This means:
- Hook events like PermissionRequest, Stop, SubagentStop, ConfigChange have response builders ready to use
- No document tells a future session these builders exist
- A session building a new hook for one of these events would write its own JSON serialization instead of using the existing typed builders

---

### S3. deploy_categories.toml has no documentation for verify methods

The deploy_categories.toml file uses verification methods (`help`, `stdin`, `exit_codes`, `python_import`) but no document explains what these mean or how to choose among them when adding a new crate. MUST_READ_BEFORE_BUILDING.md and NORNIR_CONVENTIONS.md mention `deploy_categories.toml` but do not describe its schema.

---

## Summary

| Priority | Count | Items |
|----------|-------|-------|
| P1: Doc/Code Disagreements | 13 | D1-D13 |
| P2: Missing Design Documents | 4 | M1-M4 |
| P3: Internal Contradictions | 2 | C1-C2 |
| P4: Stale Inventories | 3 | S1-S3 |

### Highest-damage items for immediate human review:

1. **D3 + C2**: HOOK_DESIGN.md PostToolUse wire format is wrong (`systemMessage` vs `hookSpecificOutput.additionalContext`). A future session building a PostToolUse hook from this document will produce output that Claude Code ignores.

2. **D7 + C1**: SYN_DESIGN.md claims the PostToolUse hook uses gate mode. It uses report mode. The same document acknowledges gate mode's ratchet is not built. A future session will waste time trying to understand why gate mode is wired but not working.

3. **M1**: The intercept/session subsystem (5 crates) has no design document. This is the most complex multi-crate subsystem in the workspace and the one most likely to be modified.

4. **D2 + D1**: The `Ask` variant exists in both `Severity` and `HookDecision` but is absent from HOOK_DESIGN.md. A future session reading the design doc will not know that interactive user confirmation is an available decision path.

5. **S2**: The response module supports 11+ event types but only 2 are documented. Future hooks for undocumented events will reimplement JSON serialization.
