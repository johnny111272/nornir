# CC_LAUNCH — Design Document

## What This Is

`cc_launch` is a ratatui TUI launcher that wraps the `claude` binary. It assembles a system prompt from a library of composable XML fragments, sets environment variables, optionally changes the working directory, and then `exec`s into `claude --system-prompt-file <assembled-file>` with any passthrough flags.

It lives at `nornir/cli/cc_launch/` within the nornir Rust workspace.

## Why This Exists

### The Problem

Anthropic's default Claude Code initialization locks Claude into a single operating mode: coding-focused, task-completion, execution-first. This makes Claude adequate at coding, mediocre at general tasks, and terrible at system design and architecture.

The user's work spans three distinct levels:
- **System Architecture** — components, data flow, interactions, boundaries.
- **System Design** — solution exploration, tradeoff comparison, approach selection.
- **Implementation** — writing code, running tests, debugging.

Anthropic's init collapses all three into "write code." This creates constant friction: Claude jumps to implementation during architecture, picks the first viable solution during design, and treats every interaction as a coding task.

### The Solution

Replace Anthropic's one-size-fits-all initialization with a composable system. The user selects a workspace, persona, and capabilities through a TUI. The launcher assembles a system prompt tailored to the session from a library of atomic XML fragments. Different sessions get different initializations.

**The system prompt IS the expertise.** A focused prompt produces a specialist. A broad prompt produces a generalist. The launcher shapes expertise through precise selection — every fragment included must be relevant to this specific session. Loading "just in case" dilutes expertise.

### Core Design Principles

**1. Everything in a prompt is a weight.** Early position = high importance. Loading instructions "just in case" actively biases behavior. Only load what's needed for this specific session.

**2. Frameworks persist, rules decay.** Under attention splitting, training data overrides rules. A logically coherent thinking framework generates correct behavior situationally. Each instruction carries a `<principle>` (the WHY) that creates internal gravity.

**3. Personas exploit training data.** "You ARE Odin" activates strategic thinking, cross-domain vision, planning-before-action from thousands of pages of Norse literature. Zero tokens spent explaining it.

**4. Collaboration is THE foundation.** If the LLM cannot collaborate, its other capabilities are worthless. The collaboration paradigm is the first thing in the system prompt, before cognitive frameworks, before personas, before anything.

**5. Derive, don't configure.** Languages come from the descriptors you select. Expertise comes from the workspace profile. The launcher figures out smart defaults — you override only when you insist.

---

## Prompt Assembly Order

The order reflects priority of importance for LLM effectiveness:

### Layer 1: Identity (who you are, how you think)

Assembled in this order within the always-loaded category:

1. **Collaboration paradigm** (`cognitive-mode/00-collaboration-paradigm.xml`) — THE foundation. Without collaboration, everything else is useless.
2. **Cognitive frameworks** (`cognitive-mode/*.xml`) — how to think: mental maps, error models, naming-as-instruction, economic pressure.
3. **Behavioral atoms** (`behavior/*.xml`) — how to act: follow-dont-lead, active reading, never invent data, say I don't know.
4. **Communication rules** (`communication/*.xml`) — response style, question format, no time estimates.
5. **Safety guardrails** (`safety/*.xml`) — activate at decision points: destructive actions, high confidence, errors.
6. **Anthropic platform mechanics** (`anthropic/*.xml`) — how CC tools work, memory, hooks. Mechanical knowledge.
7. **Persona** (`personas/*.xml`) — who you are within the collaboration.

### Layer 2: Context (where you are, what you're working on)

8. **Space descriptor** (`spaces/*.xml`) — auto-loaded when persona + workspace path match. Workflow, materials, boundaries for collaboration environments.
9. **System descriptors** (`systems/*.xml`) — manually selected. Primary first, siblings next, overview last. SOPs, stack, structure, conventions for technical systems.

### Layer 3: Mode (how you work this session)

10. **Coding fragments** (`coding/*.xml`) — only when coding is enabled. Implementation-specific atoms.
11. **Language context** — derived from selected descriptors. `<context><language>rust</language>...</context>`
12. **Expertise modules** (`expertise/*/`) — selected domain knowledge.

---

## Systems and Spaces

### Systems (`library/systems/*.xml`)

Technical system descriptors. Tag: `<workspace>`. Describe codebases, infrastructure, build pipelines. Selectable in the TUI's Systems section.

Key attributes on the root tag:
- `id` — system name
- `parent` — parent system (for family grouping)
- `languages` — comma-separated languages used by this system

Family behavior:
- **Selecting a child** (e.g., regin) → auto-selects siblings + parent overview at end
- **Selecting a parent** (e.g., nornir) → just the primary, children available but unchecked
- **Explicit `descriptors` array** in profile for exceptions (e.g., yggdrasil lists all children)

Descriptor families (via parent attribute):
- `agent-pipeline`: verdandi, draupnir, regin, galdr
- `yggdrasil`: hlidskjalf, svalinn, kvasir, ratatoskr
- `guardrail-system` (parent=nornir): gleipnir, saga, syn
- Standalone: nornir, bifrost

### Spaces (`library/spaces/*.xml`)

Collaboration environment descriptors. Tag: `<space>`. Describe workflows, materials, boundaries for thinking/planning workspaces. NOT selectable in the TUI.

Auto-load rule: if persona id matches a space id AND the workspace path contains `/spaces/{id}`, the space descriptor is automatically included. Being Bragi in nornir does NOT load the Bragi space.

### Language Derivation

Languages are derived from descriptors, never manually configured in profiles. When you select systems in the TUI, the union of all `languages` attributes from selected descriptors determines which languages are active. Adding a system automatically adds its languages. Python is always included when coding is enabled.

---

## Permissions

Permissions are superpowers — rare, deliberate path exemptions for sessions that work ON the security system.

The actual mechanism is `HOOK_LLM_ALLOW_PATHS` — a colon-separated list of paths exempt from probing/gaming hook checks. See `~/.ai/tools/scripts/start_tyr` for the real example.

**Permissions are NEVER auto-selected.** They must be manually toggled in the TUI. Most sessions need zero permissions.

**IMPORTANT:** The current `permissions.rs` contains FABRICATED env var names (GLEIPNIR_ENABLED, VOICE_ENABLED, WORKSPACE_REGISTRY_ENABLED). These are wrong and need replacing with the real `HOOK_LLM_ALLOW_PATHS` mechanism. See the pending work section.

---

## Workspace Profiles

Config file: `~/.ai/control/cc_launch_profiles.toml`

Each profile defines defaults that cascade when the workspace is selected:

```toml
[nornir]
path = "/Users/johnny/.ai/smidja/nornir"
persona = "eitri"
primary_descriptor = "nornir"
coding = true
expertise = ["functional-programming", "boundary-architecture", "schema-first"]
permissions = ["gleipnir"]
```

### Profile Fields

| Field | Purpose |
|-------|---------|
| `path` | Working directory for claude. Also used for space auto-matching. |
| `persona` | Which persona XML to load. |
| `primary_descriptor` | System descriptor that loads first. Triggers family selection. |
| `descriptors` | Additional descriptors to pre-select (e.g., yggdrasil children). |
| `coding` | Whether coding mode is enabled (default: true). |
| `expertise` | Which expertise modules to pre-select. |
| `permissions` | NOT USED for auto-selection. Parsed but ignored. Permissions are manual only. |

### Cascade Behavior

1. User selects workspace → profile fills defaults (persona, coding, expertise, descriptors)
2. Languages derived from selected descriptors (not in profile)
3. Permissions NOT auto-selected (manual only)
4. User can override ANY default before launching
5. Coding toggle: "Disable coding mode" — when disabled, languages greyed and locked
6. Selecting "Auto (current dir)" clears all profile defaults

---

## Coding Mode

Coding is ON by default. The TUI shows "Disable coding mode" — checking it turns coding OFF.

When coding is **enabled** (default):
- Coding fragments (`coding/*.xml`) loaded into prompt
- Language selectors active, derived from descriptors
- Python always included

When coding is **disabled**:
- Coding fragments NOT loaded
- Language selections persist but greyed out and unresponsive
- Languages NOT included in prompt

This is critical for non-coding sessions (architecture, design, planning, interviewing). Loading coding atoms biases the LLM toward implementation thinking.

---

## Session Lifecycle

### New Session (cc_launch → claude)

```
cc_launch TUI
  → user selects workspace, persona, systems, expertise
  → languages derived from descriptor union
  → prompt assembled in layer order (identity → context → mode)
  → writes to /tmp/cc_launch_prompt.xml
  → sets env vars for permissions (HOOK_LLM_ALLOW_PATHS)
  → exec: claude --system-prompt-file /tmp/cc_launch_prompt.xml [passthrough flags]
```

### Continuation (claude --continue)

**The system prompt is FIXED at original session creation.** `--continue` and `--resume` do NOT re-read the system prompt. The following ARE re-read from disk:

- CLAUDE.md files (project + user + global)
- Auto-memory (MEMORY.md + topic files)
- settings.json (hooks, permissions)
- MCP server configs

The SessionStart hook fires on EVERY session start, including continuations. This is a live injection point that survives continuation.

---

## Architecture

### Source Files

All at `nornir/cli/cc_launch/src/`:

| File | Purpose |
|------|---------|
| `main.rs` | CLI args via clap, scan → TUI → assemble → exec claude |
| `model.rs` | Fragment, Descriptor (parent, languages, kind), AppState, WorkspaceProfile, smart selection logic |
| `library.rs` | Scans library/ dirs, parses XML attrs via regex (id, parent, languages, load) |
| `assembly.rs` | Prompt composition in layer order, token estimation, env var building |
| `tui.rs` | 3-column ratatui TUI with scrolling, cursor tracking, panic hook |
| `profiles.rs` | Loads workspace profiles from TOML |
| `permissions.rs` | **FABRICATED** — needs replacing with real HOOK_LLM_ALLOW_PATHS mechanism |

### TUI Layout

```
┌─ Workspace ──────┐  ┌─ Systems ─────────────┐  ┌─ Session Summary ──────┐
│  ● Auto           │  │  [x] Nornir           │  │  Workspace: Nornir      │
│  ○ Bragi          │  │  [ ] Bifrost           │  │  Persona:   Eitri       │
│  ○ Nornir         │  │  [x] Gleipnir (guard.) │  │  Context:               │
│  ○ Regin          │  │  ...                   │  │    1. Nornir             │
├─ Persona ────────┤  ├─ Expertise ────────────┤  │  Coding:    rust, py    │
│  ○ None           │  │  [x] Functional Prog   │  │  Expertise: ...         │
│  ● Eitri          │  │  [ ] Voice Preserv.    │  │  Permissions: none      │
├─ Coding ─────────┤  └────────────────────────┘  │                         │
│  [ ] Disable      │                              │  Est. tokens: ~4200     │
│  [x] python       │                              └─────────────────────────┘
│  [x] rust         │
├─ Permissions ────┤
│  [ ] Gleipnir     │
└──────────────────┘
 Tab section  ↑↓ navigate  Space toggle  Enter launch  q quit
```

Column 1 (Identity): Workspace, Persona, Coding, Permissions
Column 2 (Content): Systems (scrollable), Expertise
Column 3 (Summary): Ordered context, token estimate

### Key Design Choices

- **Regex, not XML parser** — root tag attributes are predictable and on the first line
- **Deterministic temp file** — `/tmp/cc_launch_prompt.xml` overwrites each launch
- **Unix exec** — `Command::exec()` replaces the process. No parent wrapper
- **Token estimation** — `bytes / 4` heuristic for TUI display
- **Panic hook** — restores terminal from raw mode before printing panic

---

## Pending Work

### Critical
- [ ] Replace permissions.rs with real `HOOK_LLM_ALLOW_PATHS` mechanism (current env vars are fabricated)

### Library
- [ ] System architecture thinking framework
- [ ] System design thinking framework
- [ ] Expertise modules: convert from .md to two-tier XML
- [ ] Rewrite kept anthropic fragments for tone

### Launcher
- [ ] Auto-detect workspace from CWD on startup
- [ ] Warn when --continue/--resume detected in passthrough flags
- [ ] Expertise auto-derivation from descriptors (like languages)

### Integration
- [ ] SessionStart hook timing investigation
- [ ] Verify --system-prompt-file fully replaces default prompt
- [ ] ~/.ai/control/ lockdown — exemption mechanism needed

---

## Key Files

| File | Purpose |
|------|---------|
| `~/.ai/control/library/` | Fragment library root |
| `~/.ai/control/library/PROMPT_DESIGN.md` | Design philosophy document |
| `~/.ai/control/library/systems/README.md` | System descriptor format spec |
| `~/.ai/control/library/spaces/README.md` | Space descriptor format spec |
| `~/.ai/control/library/systems/DISPATCH_PROTOCOL.md` | Agent dispatch template for writing new descriptors |
| `~/.ai/control/cc_launch_profiles.toml` | Workspace profile definitions |
| `~/.ai/smidja/nornir/cli/cc_launch/` | Launcher crate |
| `~/.ai/tools/scripts/start_tyr` | Real permissions example (HOOK_LLM_ALLOW_PATHS) |
| `/tmp/cc_launch_prompt.xml` | Assembled prompt output (overwritten each launch) |

## Usage

```bash
# Launch TUI, select options, hit Enter
cc_launch

# Pass flags through to claude
cc_launch -- --model sonnet
```
