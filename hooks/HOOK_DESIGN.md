# Hook System Architecture

**Status:** Design document. Governs naming, contracts, organization, and build
patterns for all nornir hook binaries.

---

## Naming Convention

### Pattern: `hook_{event}_{context}_{scope}`

| Axis | Values | What it encodes |
|---|---|---|
| **event** | `pre`, `post`, `start`, `end`, `compact` | WHEN — maps to Claude Code hook event |
| **context** | `llm`, `subagent`, `session` | WHO — what triggered the event |
| **scope** | `tool`, `bash`, `orient`, `preserve` | WHAT — tool type or action category |

The event prefix IS the verb. `pre_` means gate. `post_` means assess. No separate
verb axis needed.

### Event mapping

| Claude Code event | Our prefix | Action | Output contract |
|---|---|---|---|
| PreToolUse | `pre_` | Gate before execution | `permissionDecision` + `additionalContext` |
| PostToolUse | `post_` | Assess after execution | `systemMessage` |
| SessionStart | `start_` | Orient at session begin | `systemMessage` |
| SessionEnd | `end_` | Finalize at session close | exit code only |
| PreCompact | `compact_` | Preserve before compaction | `systemMessage` |

### Current hooks

| Name | Event | Context | Scope |
|---|---|---|---|
| `hook_pre_llm_tool` | PreToolUse | LLM | Read/Write/Edit/Grep/Glob |
| `hook_pre_llm_bash` | PreToolUse | LLM | Bash |
| `hook_pre_subagent_tool` | PreToolUse | Subagent | Read/Write/Edit/Grep/Glob |
| `hook_pre_subagent_bash` | PreToolUse | Subagent | Bash |
| `hook_post_llm_tool` | PostToolUse | LLM | Write/Edit |
| `hook_stop_llm_tts` | Stop | LLM | TTS playback |
| `hush` | UserPromptSubmit | Session | Kill workspace announce playback |

### Planned hooks

| Name | Event | Context | Scope | Purpose |
|---|---|---|---|---|
| `hook_start_session_orient` | SessionStart | Session | — | Context injection at session begin |
| `hook_compact_session_preserve` | PreCompact | Session | — | Handover generation |

---

## IO Contracts

### Shared: stdin input

All hooks receive JSON on stdin from Claude Code:

```json
{
    "session_id": "...",
    "transcript_path": "...",
    "cwd": "...",
    "permission_mode": "...",
    "hook_event_name": "PreToolUse|PostToolUse|...",
    "tool_name": "Write|Edit|Bash|...",
    "tool_input": { "file_path": "...", ... }
}
```

PostToolUse also includes `tool_result` with the completed tool's output.

### PreToolUse output (gate)

```json
{
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "allow|deny",
        "permissionDecisionReason": "...",
        "additionalContext": "..."
    }
}
```

Handled by `hook_io::run_pre_hook(decide_fn)`.

Decision function signature: `fn(&HookInput) -> HookDecision`

Where `HookDecision` is `Allow | Warn { ... } | Deny { ... }`.

### PostToolUse output (assess)

```json
{
    "systemMessage": "..."
}
```

Handled by `hook_io::run_post_hook(assess_fn)`.

Assessment function signature: `fn(&PostHookInput) -> Option<String>`

Returns `Some(message)` to inject into LLM context, `None` for no-op.

### Side effects (all hooks)

All hooks that produce Warn/Deny/Assessment results also:
- Emit a datagram to Hlidskjalf via `datagram_io`
- Send macOS notification via `terminal-notifier`
- Append to `~/.claude/intercept.log`

These are fire-and-forget — never block the hook.

---

## hook_io Capability Crate

Single crate, two entry points plus shared rule types:

```rust
// PreToolUse
pub fn run_hook<F>(decide_fn: F) -> ExitCode
where F: FnOnce(&HookInput) -> HookDecision;

// PostToolUse
pub fn run_post_hook<F>(assess_fn: F) -> ExitCode
where F: FnOnce(&PostHookInput) -> Option<String>;
```

### hook_io::rules module

Shared rule parsing extracted from hook binaries:

```rust
pub enum Severity { Warn, Block }
pub fn parse_severity(s: &str) -> Option<Severity>;

pub struct RawRule { pub pattern: String, pub description: String }
pub fn parse_rule_array(table: &toml::Table, key: &str) -> Vec<RawRule>;
pub fn parse_toml_table(toml_str: &str) -> Result<toml::Table, String>;
```

All pre hooks use this to parse their embedded `rules.toml` at startup. `hook_pre_llm_tool` uses `RawRule.pattern` as substring match. `hook_pre_llm_bash` compiles patterns as regex via `CompiledRule::from_raw()`.

### Shared internals

- `read_stdin()` — parse JSON input
- `emit_to_watchtower()` — datagram emission
- `notify_and_log()` — macOS notification + log append

### Type additions

```rust
pub struct PostHookInput {
    pub tool_name: Option<String>,
    pub tool_input: serde_json::Value,
    pub tool_result: Option<serde_json::Value>,
}
```

---

## Dispatch Architecture

### Pre hooks: rule-based dispatch

Pre hooks match rules against the tool input and return a decision.
Dispatch is pattern matching on the input content (file paths, command strings).
All logic is internal — no subprocess calls.

### Post hooks: runner-based dispatch

Post hooks classify the input and delegate to an appropriate runner.
Each runner is a function that orchestrates external tools via subprocess.
The hook binary is thin — dispatch + orchestration, not analysis.

```
hook_post_llm_tool
    |
    +-- dispatch on tool_name + file extension:
    |
    +-- .py  --> assess_python(file_path)
    |             saga <file> --sidecar | syn --stdin
    |             captures syn stdout as systemMessage
    |             syn broadcasts datagram as side effect
    |
    +-- .rs  --> None (future: cargo check)
    +-- .toml -> None (future: schema validation)
    +-- _    --> None (no-op)
```

### Runner separation principle

Post hook runners shell out to external tools. They do NOT link against
analysis libraries (saga_runner, syn internals). This maintains the decoupled
intermediate-file architecture:

- saga writes .qa AND emits JSON to stdout
- syn reads JSON from stdin AND broadcasts datagram
- The hook only knows how to pipe them together

Hook dependencies: `hook_io` + `serde_json`. Nothing else.

---

## Build and Deploy

### Workspace members

All hooks are workspace members in `nornir/Cargo.toml`:

```toml
"hooks/hook_pre_llm_tool",
"hooks/hook_pre_llm_bash",
"hooks/hook_pre_subagent_tool",
"hooks/hook_pre_subagent_bash",
"hooks/hook_post_llm_tool",
```

### Deploy

`deploy_hooks.py` builds release binaries and symlinks into `~/.ai/tools/bin/`.
Binary names match crate bin names exactly — no renaming at deploy.

### Settings.json wiring

```json
{
    "PreToolUse": [
        {
            "matcher": "Read|Grep|Glob|Write|Edit",
            "hooks": [{ "type": "command", "command": "hook_pre_llm_tool --probing warn --gaming warn" }]
        },
        {
            "matcher": "Bash",
            "hooks": [{ "type": "command", "command": "hook_pre_llm_bash --subversion block --truncation warn --evasion warn" }]
        }
    ],
    "PostToolUse": [
        {
            "matcher": "Write|Edit",
            "hooks": [{ "type": "command", "command": "hook_post_llm_tool", "timeout": 30000 }]
        }
    ]
}
```

---

## Rename Migration — COMPLETE

The four `hook_intercept_*` hooks were renamed to `hook_pre_*`.

Completed: directories, Cargo.toml names, workspace members, deploy_hooks.py,
settings.json, old symlinks removed. All verified.

---

## Pipeline: PostToolUse Quality Assessment

The first PostToolUse hook. End-to-end flow:

```
Claude writes .py file
    |
    v
PostToolUse fires
    stdin = { tool_name: "Write", tool_input: { file_path: "/path/to/file.py" }, ... }
    |
    v
hook_post_llm_tool
    |
    +-- extract file_path from stdin JSON
    +-- filter: .py only, file must exist
    +-- spawn: saga <file_path> --sidecar
    |     saga writes .qa sidecar to disk (Svalinn reads later)
    |     saga emits .qa JSON to stdout
    +-- pipe saga stdout to: syn --stdin
    |     syn filters, groups, formats TOON
    |     syn broadcasts datagram to Hlidskjalf (side effect)
    |     syn emits TOON to stdout
    +-- capture syn stdout
    +-- return: { "systemMessage": "<TOON assessment>" }
    |
    v
Claude receives quality assessment in context window
Hlidskjalf displays assessment on dashboard
Svalinn can read .qa sidecar at any time
```

### Timeout budget

settings.json allows 30s. Budget allocation:
- saga: ~20s (runs ruff + basedpyright + gleipnir as subprocesses)
- syn: <1s (reads JSON, filters, formats, broadcasts)
- hook overhead: <100ms

If saga exceeds budget, the hook times out and Claude continues without
assessment. This is acceptable — quality feedback is informational, not blocking.
The .qa sidecar still gets written (saga completes in background even if the
hook times out on capturing stdout).

### Future: PreToolUse quality gate

Once the PostToolUse assessment is stable, a PreToolUse gate can use the same
pipeline to BLOCK writes that regress quality:

```
hook_pre_llm_tool (extended)
    +-- on Write/Edit of .py:
    |     saga --stdin <proposed content> | syn --mode gate --stdin
    |     syn compares against baseline .qa (ratchet)
    |     exit 1 = deny (regression detected)
```

This requires syn's ratchet comparison engine (not yet built).
