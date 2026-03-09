# Syn Design — Quality Policy Gate

**Named after Old Norse "syn" — denial, refusal. The gatekeeper.**

Syn sits between Saga (truth recorder) and the consumer (LLM, human, CI).
Saga records ALL issues. Syn applies policy — filtering, comparison, ratcheting.

---

## Architecture

```
saga (truth)  →  .qa JSON  →  syn (policy)  →  filtered output + decision
```

Syn has TWO fundamentally different modes:

- **Report mode**: view into current quality state (informational)
- **Gate mode**: per-file ratchet comparison (enforcement)

---

## Core Concepts

### Saga Produces Truth

Saga runs quality tools (gleipnir, ruff, basedpyright) on Python files and
writes `.qa` sidecar files. These contain every issue found — no filtering,
no policy. Just raw truth.

### Syn Applies Policy

Syn reads .qa truth and makes decisions. In report mode, it filters and
formats for human/LLM consumption. In gate mode, it compares the new state
against the baseline and enforces the ratchet — quality never regresses.

### The Ratchet (Per-File Baseline Comparison)

The breakthrough insight: rather than enforcing project-wide quality settings,
enforce **per-file** using the old file state as baseline. Each file has its
own .qa sidecar representing its last accepted state. Every edit is judged
relative to that file's own history.

This works WITH LLM behavior instead of against it. LLMs handle local
optimization well but struggle with global constraints. The ratchet gives
them a local target: don't make THIS file worse.

---

## Filter Layers

Two filter layers define what matters. Both use jq expressions evaluated
by jaq-core against each issue object.

### `.syn/warn.toml` — Noise Filter

Defines what issues are visible (worth showing). Everything that fails
this filter is suppressed as noise — we don't even bother reporting it.

```toml
filter = '.tool == "gleipnir"'
```

Default (no file): `.tool == "gleipnir"`

### `.syn/deny.toml` — Quality Standard

Defines the final quality standard. Issues passing this filter represent
the threshold where code has met or exceeded the standard.

```toml
filter = '.severity == "blocked"'
```

Default (no file): `.severity == "blocked"`

### How Filters Relate to Ratchets

The ratchet operates on issues that pass the **warn** filter (visible issues).
The **deny** filter defines the final standard — once a file has zero deny-level
issues, it has met the standard.

```
all issues from .qa
  │
  ├─ fails warn filter ──→ SUPPRESSED (noise, not counted)
  │
  └─ passes warn filter ──→ VISIBLE (counted for ratchet comparison)
       │
       └─ also passes deny filter ──→ STANDARD (the bar to clear)
```

---

## Modes

### `--mode report` (default)

Informational view into current quality state. No gate decision.
CLI overrides allowed for exploration.

1. Load .qa report(s) from path, directory, or `--project-dir`
2. Apply warn filter (or CLI override via --tool/--level/--filter)
3. Format output (TOON on pipe, colored on tty, or --output json)
4. Broadcast to Hlidskjalf (default on, --silent to suppress)
5. Exit 0 always

### `--mode gate`

Per-file ratchet enforcement. Deterministic. Locked to config.
**Rejects --tool/--level/--filter flags with error.** Used by hooks.

Two inputs required:
1. **Baseline .qa** — existing sidecar on disk (last accepted state)
2. **New .qa** — from saga running on the proposed file content

The comparison:

```
baseline .qa (old)  vs  new .qa (proposed)
         │                    │
         └── visible issues ──┘
                   │
                   ▼
            ratchet check
```

### Ratchet Thresholds

Configured in `.syn/ratchet.toml`:

```toml
mode = "no_regression"   # default
```

Three ratchet modes:

| Mode | Rule | Meaning |
|------|------|---------|
| `no_regression` | new ≤ baseline | Don't make it worse |
| `improvement` | new < baseline (while issues remain) | Every save must improve |
| `meets_standard` | zero deny-level issues | Must meet or exceed the quality standard |

- **no_regression**: The gentlest ratchet. You can't introduce new visible
  issues, but you don't have to fix existing ones. Quality holds steady
  or improves.

- **improvement**: Every edit must reduce the visible issue count, as long
  as there are still issues to fix. Once at zero, holds at zero.

- **meets_standard**: The file must have zero issues that pass the deny
  filter. Quality may still vary within the warn range, but never dips
  under the deny standard.

### Gate Decision

- **Exit 0**: Accept — ratchet check passed
- **Exit 1**: Reject — regression detected or standard not met
- **Exit 2**: Usage error (bad flags, missing input)

Output includes: all visible issues (warn-level), the ratchet comparison
result, and the decision with reason.

---

## Input Sources

### Report Mode

Reads existing .qa sidecars:

1. **File path** — `syn structures.py` → finds `.structures.py.qa` sidecar
2. **Directory** — `syn src/` → walks for all `.qa` files
3. **`--project-dir`** — `syn --project-dir /path/to/bragi` → scan that project
4. **No args** — cwd

### Gate Mode

Needs two things:

1. **File path** (positional arg) — used to find the baseline .qa sidecar
2. **New .qa on stdin** — fresh saga output for the proposed content

```
saga <file.py> --stdin | syn --mode gate <file.py>
```

Or in a hook context, the hook runs saga on the new content and pipes
the .qa JSON to syn, while also passing the file path so syn can find
the baseline sidecar.

If no baseline exists (new file), the ratchet has nothing to compare
against — the file starts with its first .qa as baseline.

---

## Config Discovery

Syn discovers its config by walking **up the directory tree** from the
input path, looking for a `.syn/` directory. This is the same pattern
gleipnir uses to find `.gleipnir/`.

```
given input: /Users/johnny/.ai/bragi/src/bragi/structures.py
walks up:
  /Users/johnny/.ai/bragi/src/bragi/.syn/  — no
  /Users/johnny/.ai/bragi/src/.syn/         — no
  /Users/johnny/.ai/bragi/.syn/             — YES → load config from here
```

The walk starts from:
- The **positional path** if one is given (file or directory)
- The **`--project-dir`** path if no positional path
- **cwd** if neither

Once `.syn/` is found, syn loads whatever config files exist there.
Missing files use defaults. No `.syn/` directory at all = all defaults.

---

## CLI Interface

```
syn [options] [path]

Modes:
  --mode report    Informational (default). CLI overrides allowed.
  --mode gate      Ratchet enforcement. Locked to config. Needs stdin.

Input:
  <path>           File or directory (positional)
  --project-dir    Project root for hook-triggered scans (see below)
  --target         [src|tests|all] Subtree scope (default: src)

Output:
  --output         [colored|json|toon] (default: colored on tty, toon on pipe)
  --silent         Suppress Hlidskjalf broadcast

Filters (report mode only, rejected in gate mode):
  --tool           [gleipnir|ruff|basedpyright|all]
  --level          [info|warning|error|blocked] and above
  --filter         '<jq expression>'
```

### `--project-dir` — Hook Scan Target

`--project-dir` gives syn a project directory to scan when there is no
positional path. This exists for **hook-triggered reports** where the
hook has no file context — only a project directory extracted from
Claude's JSON session data.

Functionally, `syn --project-dir /path/to/bragi` and `syn /path/to/bragi`
produce the same result. The explicit flag is a semantic distinction:
it signals "I am a hook injecting context" rather than "I am a human
pointing at a directory." This makes hook scripts self-documenting,
which matters when LLMs are authoring those hooks.

---

## Config Files

Located in `.syn/` discovered by walking up the directory tree.

| File | Purpose | Default |
|------|---------|---------|
| `warn.toml` | Noise filter — what's visible | `.tool == "gleipnir"` |
| `deny.toml` | Quality standard — the bar | `.severity == "blocked"` |
| `ratchet.toml` | Ratchet mode | `mode = "no_regression"` |

---

## Output

**Stdout** (mutually exclusive):
- **TOON** (default on pipe) — token-optimized for LLM context (~40% fewer tokens)
- **Colored** (default on tty) — ANSI terminal output for humans
- **JSON** (`--output json`) — machine-readable

**Hlidskjalf broadcast** (independent side channel):
- ON by default — emits JSON to Hlidskjalf unix socket via socket_emit
- `--silent` to suppress
- Fire-and-forget: no cost if Hlidskjalf isn't listening
- Payload includes: issues, ratchet comparison, decision, workspace

---

## Integration Points

### PostToolUse Hook (current)

After every Python file write:
1. Saga runs on the written file → produces fresh .qa
2. Syn gate mode: compares fresh .qa against baseline sidecar
3. Decision returned as additionalContext (TOON format)
4. Broadcast to Hlidskjalf

### PreToolUse Hook (future)

Before a write is accepted:
1. Hook receives proposed file content from tool_input
2. Runs saga on proposed content → gets .qa JSON
3. Pipes to syn gate mode with file path
4. Syn compares against baseline, returns accept/reject
5. Hook returns permissionDecision based on exit code

### Session-Start Report (hook-triggered)

On session initialization or other trigger events:
1. Hook extracts project directory from Claude's session JSON
2. Runs `syn --project-dir <dir> --target src --output toon`
3. Full project quality report injected into LLM context
4. No file path needed — `--project-dir` provides the scan root

---

## Dependencies

### Already Built
- **format_core** — JSON, YAML, TOML, TOON parse/serialize/convert (74 tests)
- **TOMLX parser** — 7 files, 42 tests
- **error_core** — educational error types (10 tests)
- **saga_core** — generates .qa reports, canonical SanityReport/Issue types, directory walker (21 tests)
- **report_render_core** — grouping, formatting, severity ordering for QA consumers (38 tests)
- **socket_emit** — fire-and-forget Hlidskjalf broadcast
- **jaq-interpret 1.5** — embedded jq filter evaluation

### Current State (Phase 1 DONE)
- Report mode: working (filter, format, broadcast) — 43 tests
- Gate mode: filtering works, decision logic works, **ratchet comparison NOT YET IMPLEMENTED**
- Deployed to `~/.ai/tools/bin/syn`
- Pure rendering logic extracted to `core/report_render_core/` (38 tests) — shared with svalinn
- syn is 478 lines (down from 909 after report_render_core extraction)
- hook_post_llm_tool wired: saga → syn pipeline operational

### Remaining
- Ratchet comparison engine (baseline vs new)
- Ratchet config loading (`.syn/ratchet.toml`)
- Gate mode stdin input for new .qa baseline comparison
- PreToolUse quality gate (future, needs ratchet)
