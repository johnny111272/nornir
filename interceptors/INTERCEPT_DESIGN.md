# Intercept Pipeline Design

**Status:** Current as of 2026-03-19. Governs the traffic intercept and session
file subsystem.

---

## Purpose

Capture every Claude API request passing through bifrost, classify it by tool
composition, and route it to structured JSONL files per session. Enable
compaction-aware session reconstruction from the raw capture log.

---

## Crate Boundaries

```
Tier 1 (pure):
    intercept_core    — ExchangeKind, classify_exchange, has_tool, tool_count
    compaction_inject_core — inject_compaction_system_block (value mutation only)

Tier 2 (I/O):
    session_io        — append_exchange, append_subagent, append_compaction,
                        record_compaction, capture_precompaction, truncate_mainexch

Tier 3 (binaries):
    traffic_interceptor_rewriter — PyO3 module (live interception)
    intercept_replay             — CLI binary (offline reconstruction)

Related:
    watch_and_diff_exchange_intercepts — watcher binary, spawned by replay
    diff_core                         — line-level diff used by watcher
```

### Why session_io was extracted

The live interceptor (`traffic_interceptor_rewriter`) and the replay binary
(`intercept_replay`) must produce identical derived files from the same input.
Before extraction, session file writing was inline in the interceptor. This meant
replay had to either import from a PyO3 binary (architectural violation) or
reimplement the logic (guaranteed drift).

`session_io` is the single code path for all derived file writes. Both consumers
call the same functions. If session file format changes, it changes once.

### Why intercept_core is separate from session_io

Classification is pure — no filesystem, no network, no side effects. It belongs
in core/. Session I/O depends on `write_engine` and `datagram_io` for fsync and
datagram emission. Keeping classification pure means it can be tested with
in-memory JSON values and used in contexts that don't need I/O.

---

## Directory Layout

```
~/ai/intercept/
  sessions/
    {session_id}/
      raw_session_log.jsonl         # verbatim request bytes (live only)
      main_exchange_log.jsonl       # classified main-agent exchanges
      subagent_log.jsonl            # classified subagent exchanges
      compaction_instructions.jsonl # compaction request bodies
      pre_compact_state.jsonl       # last main exchange before each compaction
      running_transcript.jsonl      # watcher-produced diffs
      archive/                      # replay archives prior derived files here
        {timestamp}/
  traffic/
    {workspace}/
      {session_id} → ../../sessions/{session_id}   # bifrost creates these
```

### File ownership

| File | Written by | Purpose |
|------|-----------|---------|
| `raw_session_log.jsonl` | `traffic_interceptor_rewriter` only | Verbatim capture, pre-validation |
| `main_exchange_log.jsonl` | `session_io::append_exchange` | Main agent exchanges |
| `subagent_log.jsonl` | `session_io::append_subagent` | Subagent exchanges |
| `compaction_instructions.jsonl` | `session_io::append_compaction` | Compaction request bodies |
| `pre_compact_state.jsonl` | `session_io::capture_precompaction` | Snapshot before truncation |
| `running_transcript.jsonl` | `watch_and_diff_exchange_intercepts` | Line-level diffs |
| `archive/` | `intercept_replay` | Timestamped backup of prior derived files |

---

## Classification

`intercept_core::classify_exchange` inspects the `tools` array:

| Signal | Classification |
|--------|---------------|
| No tools | `None` — skip (title generation, utility calls) |
| Exactly 1 tool = `Read` | `Compaction` |
| Has `Task` or `ToolSearch` or `EnterPlanMode` or `ExitPlanMode` | `Main` |
| Everything else | `Subagent` |

Two CC model formats exist:
- **Old** (claude-opus-4-6): 28-tool set, `Task` is the main-agent signal
- **New** (claude-sonnet-4-6): deferred tool loading, `ToolSearch` + plan mode tools are signals

---

## Live Pipeline (traffic_interceptor_rewriter)

PyO3 module imported by bifrost's `intercept_and_route.py`. No subprocess.

```
bifrost calls process(json_bytes, session_id, workspace, intercept_dir)
    |
    +-- append_raw(bytes)              # unconditional, pre-validation
    +-- validate against cc_wire_schema
    +-- classify_exchange(value)
    |
    +-- Main      → session_io::append_exchange
    +-- Subagent  → session_io::append_subagent
    +-- Compaction → handle_compaction:
    |       session_io::record_compaction   (shared: capture + write + truncate + datagram)
    |       inject_compaction_system_block   (live only: mutate request body)
    |       return rewritten bytes to bifrost
    +-- None      → skip (return None)
```

Returns `{"ok": bool, "rewritten": bytes | None, "error": str | None}`.
`rewritten` is non-None only for compaction requests — bifrost replaces the
request body before forwarding to Anthropic.

### Why append_raw is live-only

Raw capture happens before schema validation. If the CC wire format changes and
validation fails, the raw bytes are still preserved — enabling replay once the
schema is updated. Replay reads from this file, so it doesn't re-create it.

---

## Replay Pipeline (intercept_replay)

Offline binary that reconstructs derived files from `raw_session_log.jsonl`.

```
intercept_replay --session-dir <path> [--workspace <name>]
    |
    +-- archive existing derived files to archive/{timestamp}/
    +-- spawn watch_and_diff_exchange_intercepts
    +-- for each line in raw_session_log.jsonl:
    |       validate against cc_wire_schema (skip invalid)
    |       classify_exchange (skip unclassifiable)
    |       route through session_io (same as live)
    |       sleep 10ms (let watcher keep up)
    +-- kill watcher
    +-- print summary
```

### Why replay skips inject_compaction_system_block

The live interceptor injects a compaction system block because bifrost needs
to forward a modified request to Anthropic. Replay is not forwarding anything —
it's reconstructing files from historical data. The compaction instruction body
is already captured in `compaction_instructions.jsonl`. Injecting would corrupt
the historical record.

### Why replay uses cc_wire_schema validation

Raw logs may contain lines from before a CC format change. Schema validation
lets replay silently skip lines it can't process rather than crashing on
unexpected shapes. The skip count is reported in the summary.

---

## Compaction Sequence

When a compaction is detected (single Read tool):

```
1. capture_precompaction:
   - Read last line of main_exchange_log.jsonl
   - Append it to pre_compact_state.jsonl
   - Return line number (1-based) for datagram reference

2. append_compaction:
   - Write compaction request body to compaction_instructions.jsonl

3. truncate_mainexch:
   - Overwrite main_exchange_log.jsonl with just its last line
   - Uses write_truncate_fsync to preserve inode (open fd holders keep working)

4. emit datagram:
   - Alert to Hlidskjalf with pre_compact_state reference

5. (live only) inject_compaction_system_block:
   - Mutate the request value to include compaction summary instructions
   - Return rewritten bytes to bifrost
```

Steps 1-4 are in `session_io::record_compaction` — shared between live and replay.
Step 5 is in `traffic_interceptor_rewriter::handle_compaction` — live only.

### Why truncation preserves the last line

After compaction, Claude's context window resets. The main exchange log should
reflect what Claude currently knows. Keeping the last pre-compaction exchange
provides continuity — the watcher can still diff against it. Previous exchanges
are preserved in `pre_compact_state.jsonl`.

---

## Schema Validation

The CC wire schema (`cc_wire_schema.json`) is embedded at compile time in both
`traffic_interceptor_rewriter` and `intercept_replay`. The schema lives in
`interceptors/traffic_interceptor_rewriter/`; replay uses a symlink.

The schema validates the structure of Claude API requests. Key constraint:
the `tools` field uses `oneOf` to allow both function tools
(`name`/`description`/`input_schema`) and server tools (`type`/`name`/`max_uses`).

If the CC wire format changes and validation fails:
- **Live**: returns an error to bifrost (which logs and continues)
- **Replay**: silently skips the line

---

## Deploy

```bash
nornir_deploy --build interceptors
```

This builds `intercept_replay` (cargo) and `traffic_interceptor_rewriter` (maturin).
The maturin build extracts the `.so` to `~/ai/tools/lib/` and re-signs it with
`codesign -f -s -` (required on macOS — unsigned `.so` triggers SIGKILL).

After deploying `traffic_interceptor_rewriter`, restart mitmproxy to pick up the
updated module.
