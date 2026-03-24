# CC_LAUNCH — Design Document

## What This Is

`cc_launch` is a ratatui TUI launcher that wraps the `claude` binary. It assembles a system prompt from a library of composable XML fragments, sets environment variables, optionally changes the working directory, and then `exec`s into `claude --system-prompt-file <assembled-file>` with any passthrough flags.

It lives at `nornir/cli/cc_launch/` within the nornir Rust workspace.

## Why This Exists

### The Problem

Anthropic's default Claude Code initialization locks Claude into a single operating mode: coding-focused, task-completion, execution-first. This makes Claude good at coding, mediocre at general tasks, and terrible at system design and architecture.

The user's work spans three distinct levels:
- **System Architecture** — components, data flow, interactions, boundaries. High-level orchestration.
- **System Design** — solution exploration, tradeoff comparison, approach selection. The middle level where multiple viable approaches are held open and compared before committing.
- **Implementation** — writing code, running tests, debugging. Building what was designed.

Anthropic's init collapses all three into "write code." This creates constant friction: Claude jumps to implementation during architecture, picks the first viable solution during design, and treats every interaction as a coding task.

### The Solution

Replace Anthropic's one-size-fits-all initialization with a composable system. The user selects a workspace, persona, and capabilities through a TUI. The launcher assembles a system prompt tailored to the session from a library of atomic XML fragments. Different sessions get different initializations.

### Core Design Principles

**1. Everything in a prompt is a weight.** Early position = high importance. Loading instructions "just in case" actively biases behavior. Only load what's needed for this specific session.

**2. Frameworks persist, rules decay.** Under attention splitting, training data overrides rules. A logically coherent thinking framework generates correct behavior situationally, including for unenumerated cases. Each instruction carries a `<principle>` (the WHY) that creates internal gravity.

**3. Personas exploit training data.** "You ARE Odin" activates strategic thinking, cross-domain vision, planning-before-action from thousands of pages of Norse literature. Zero tokens spent explaining it. The mythology IS the thinking framework, loaded for free.

**4. Vocabulary becomes behavioral triggers.** Terms defined in the init ("architect", "design", "collaborate to") activate specific cognitive modes during conversation without needing runtime rules.

---

## The Three-Axis Model

The initialization system operates across three independent axes. See `PROMPT_DESIGN.md` in `~/.ai/control/library/` for full details.

### Axis 1: Cognitive Mode (how to perceive)
Thinking frameworks always loaded. Shapes all reasoning.
- "Everything is a Map" — clean structure = free mental map
- "Everything is Documentation" — clear naming = free embedded documentation

### Axis 2: Paradigm (how we're working together)
Flows through gates: **Collaboration** → **Planning** → **Execution**
- Collaboration is the default. Open-ended thinking together.
- Planning: Claude structures, user approves. Explicit gate.
- Execution: semi-autonomous, build what was approved.
- Transitions are user-controlled, not Claude-initiated.

### Axis 3: Work Mode (what altitude)
- **Architect** — components and connections. Feasibility dips allowed, then back up.
- **Design** — multiple approaches held open, tradeoffs compared, optimal selected.
- **Implement** — build the designed thing. Follow the design, don't redesign.

Work Mode and Paradigm are NOT pre-selected in the launcher. They are always-loaded frameworks that activate through conversation vocabulary ("let's architect this", "collaborate to design").

---

## The Fragment Library

Located at `~/.ai/control/library/`. Contains composable atomic fragments in XML format.

### Directory Structure

```
control/library/
├── behavior/          13 .xml — always loaded, mode-independent behavioral invariants
├── communication/      4 .xml — always loaded, how information flows
├── safety/             6 .xml — always loaded, decision-point guardrails
├── cognitive-mode/     4 .xml — always loaded, thinking frameworks
├── anthropic/          3 .xml — always loaded, CC platform mechanics
├── coding/             5 .xml — conditional, only when coding enabled
├── personas/           8 .xml — one selected per session (or none)
├── expertise/         10 subdirs with .md files — selectable domain knowledge
├── PROMPT_DESIGN.md       — design philosophy document
├── RAW.md                 — raw Anthropic system prompt (reference)
└── EXTRACTION_LOG.md      — provenance tracking
```

### Fragment XML Format

Instructions use `<instruction>` with `<principle>`, `<therefore>`, `<example>`:

```xml
<instruction category="behavior" id="follow-dont-lead" load="always">
  <principle>
    The WHY — the argument that persists under attention splitting.
  </principle>
  <therefore>
    The behavioral implication. What to do, grounded in the reasoning.
  </therefore>
  <example>
    <good>Correct behavior</good>
    <bad>The failure mode</bad>
  </example>
</instruction>
```

Personas use `<persona>` with `<identity>`, `<context>`, `<collaboration>`:

```xml
<persona id="odinn" archetype="All-Father">
  <identity>You ARE Odin — the All-Father...</identity>
  <context><item>You design paths — others walk them</item></context>
  <collaboration><item>We think architecturally together</item></collaboration>
</persona>
```

Platform mechanics use `<platform>` with content-appropriate inner tags.

### Always-Loaded Categories

| Category | Count | Purpose |
|----------|-------|---------|
| cognitive-mode | 4 | Thinking frameworks — mental map, naming, error model, economic pressure |
| behavior | 13 | Invariants — collaboration paradigm, consistency, follow-dont-lead, stop-on-failure, active reading, audit rigor, say-i-dont-know, etc. |
| communication | 4 | Info flow — style, no time estimates, question format, learning style |
| safety | 6 | Guardrails — error traceback, context exhaustion, confidence check, data safety, no fabricated URLs, action reversibility |
| anthropic | 3 | CC platform — system mechanics, tool usage, auto memory |

### Conditionally-Loaded

| Category | Condition | Purpose |
|----------|-----------|---------|
| coding | Coding toggle ON | Implementation atoms — refactoring, script hygiene, test integrity, self-verifying software, security awareness |
| expertise | User selection or workspace profile cascade | Domain knowledge — functional programming, boundary architecture, schema-first, security mindset, voice preservation, knowledge synthesis, etc. |
| personas | User selection (one or none) | Mythological archetype that shapes collaboration style |

---

## Workspace Profiles

Config file: `~/.ai/control/cc_launch_profiles.toml`

Each profile defines defaults that cascade when the workspace is selected in the TUI:

```toml
[nornir]
path = "/Users/johnny/.ai/smidja/nornir"
persona = "eitri"
coding = true
languages = ["rust", "python"]
expertise = ["functional-programming", "boundary-architecture", "schema-first", "security-mindset", "data-pipeline", "anti-rigidity"]
permissions = ["gleipnir"]
```

### Cascade Behavior

1. User selects workspace → profile fills all defaults (persona, coding, languages, expertise, permissions)
2. User can override ANY default before launching
3. Coding toggle auto-selects/deselects coding-related expertise
4. Python is always selected (cannot be deselected)
5. Selecting "Auto (current dir)" clears all profile defaults

### Workspace CD

If a workspace with a `path` is selected, `cc_launch` sets `current_dir` on the claude process before exec. Claude starts in the workspace directory regardless of where cc_launch was invoked.

---

## Session Lifecycle and Injection Points

### New Session (cc_launch → claude)

```
cc_launch TUI
  → user selects workspace, persona, coding, expertise, permissions
  → assembles system prompt from library XML
  → writes to /tmp/cc_launch_prompt.xml
  → sets env vars for permissions
  → exec: claude --system-prompt-file /tmp/cc_launch_prompt.xml [passthrough flags]
```

### Prompt Assembly Order

1. Selected persona (if any)
2. cognitive-mode fragments (always)
3. behavior fragments (always)
4. communication fragments (always)
5. safety fragments (always)
6. anthropic platform fragments (always)
7. coding fragments (if coding enabled)
8. Language context block (selected languages)
9. Selected expertise modules

### Continuation (claude --continue)

**The system prompt is FIXED at original session creation.** `--continue` and `--resume` do NOT re-read the system prompt. The following ARE re-read from disk:

- CLAUDE.md files (project + user + global)
- Auto-memory (MEMORY.md + topic files)
- settings.json (hooks, permissions)
- MCP server configs

The SessionStart hook fires on EVERY session start, including continuations. This is a live injection point that survives continuation.

**Implication:** cc_launch only applies to NEW sessions. For continuations, the user runs `claude --continue` directly. Any mid-session corrections that need to survive continuation should go through CLAUDE.md, memory, or the SessionStart hook — not the system prompt.

---

## Architecture

### Crate Structure

```
nornir/cli/cc_launch/
├── Cargo.toml
└── src/
    ├── main.rs          (75 lines)  — CLI args, run(), exec into claude
    ├── model.rs         (224 lines) — Fragment, Library, AppState, Section, WorkspaceProfile
    ├── library.rs       (235 lines) — Scan library dir, regex-parse XML metadata
    ├── assembly.rs      (130 lines) — Prompt composition, token estimation, temp file
    ├── profiles.rs      (74 lines)  — Load workspace profiles from TOML
    ├── permissions.rs   (23 lines)  — Known permission/env-var definitions
    └── tui.rs           (513 lines) — Ratatui event loop, layout, rendering
```

### Dependencies

- `write_engine` — only for `ai_home()` path resolution
- `clap` — CLI arg parsing with trailing var arg for passthrough
- `ratatui` + `crossterm` — TUI framework
- `regex` — XML root tag attribute extraction
- `toml` — workspace profile parsing

### Key Design Choices

- **Regex, not XML parser** — root tag attributes are predictable and on the first line. Regex is sufficient. Full file content included verbatim in assembled prompt.
- **Deterministic temp file** — `/tmp/cc_launch_prompt.xml` overwrites on each launch. No accumulation.
- **Unix exec** — `Command::exec()` replaces the process. No parent wrapper. Clean terminal handoff.
- **Token estimation** — `bytes / 4` heuristic. Sufficient for ballpark display in TUI.
- **Panic hook** — restores terminal from raw mode before printing panic. Standard ratatui practice.

### TUI Layout

```
┌─ Workspace ──────────┐  ┌─ Session Summary ──────────┐
│  ● Auto (current dir) │  │                            │
│  ○ Bragi              │  │  Workspace: Nornir          │
│  ○ Mimir              │  │  Persona:   Eitri           │
│  ○ Nornir             │  │  Coding:    enabled (rust)  │
│  ○ ...                │  │  Expertise: ...             │
├─ Persona ────────────┤  │  Permissions: gleipnir      │
│  ○ None               │  │                            │
│  ● Eitri              │  │  Always loaded:             │
│  ○ ...                │  │    30 fragments             │
├─ Coding ─────────────┤  │                            │
│  [x] Enable coding    │  │  Est. tokens: ~4200        │
│  [x] python (always)  │  │                            │
│  [x] rust             │  └────────────────────────────┘
│  [ ] typescript       │
├─ Expertise ──────────┤
│  [x] Functional Prog  │
│  [x] Boundary Arch    │
│  [ ] Voice Preserv.   │
├─ Permissions ────────┤
│  [x] Gleipnir         │
│  [ ] Voice/TTS        │
└────────────────────────┘
 Tab section  ↑↓ navigate  Space toggle  Enter launch  q quit
```

---

## What's Been Done

### Library (control/library/)
- [x] Anthropic prompt fractured into 9 atomic pieces, 6 absorbed into our categories, 3 kept as platform mechanics
- [x] 23 original behavior atoms reorganized into 5 directories (behavior, communication, safety, cognitive-mode, coding)
- [x] All atoms converted to XML format with principle/therefore/example
- [x] 5 new atoms created: active-reading, audit-means-rigorous, if-it-seems-obvious, say-i-dont-know, no-fabricated-urls
- [x] 3 new safety atoms: action-reversibility (with trash-over-rm), prefer-editing-over-creating, read-before-modifying, security-awareness
- [x] 8 personas converted to lean XML format
- [x] PROMPT_DESIGN.md capturing the full design philosophy
- [x] Workspace profiles config created

### Launcher (cc_launch)
- [x] Ratatui TUI with 5 sections: Workspace, Persona, Coding, Expertise, Permissions
- [x] Workspace profile cascade (select workspace → all defaults filled)
- [x] Language multi-select (Python always on)
- [x] Coding toggle auto-selects coding expertise
- [x] Prompt assembly in correct order
- [x] Token estimation in summary panel
- [x] Workspace cd before exec
- [x] Deployed to ~/.ai/tools/bin/cc_launch

## What's NOT Done

### Library Gaps
- [ ] System architecture thinking framework (equivalent depth to coding_reference/ docs)
- [ ] System design thinking framework
- [ ] Code planning thinking framework ("first viable ≠ best viable", hold options open)
- [ ] Expertise modules not yet converted to XML
- [ ] PROMPT_DESIGN.md open work items need updating (some are done)

### Launcher Features
- [ ] Auto-detect workspace from CWD on startup (pre-select matching profile)
- [ ] Warn or skip prompt assembly when --continue/--resume detected in passthrough flags
- [ ] Persist last-used selections per workspace (remember previous choices)
- [ ] Scrolling for long lists in small terminals

### Integration
- [ ] SessionStart hook timing investigation — does it fire before or after CLAUDE.md is read?
- [ ] Mid-session injection system — stripping system-reminders and replacing with library fragments
- [ ] Expertise tier system — brief XML reminders (in prompt) vs full reference material (loaded interactively on demand)

### Anthropic Prompt
- [ ] Rewrite kept anthropic fragments for tone (remove coding-first framing)
- [ ] Verify --system-prompt-file fully replaces default prompt (or does Anthropic's still layer on top?)

---

## Key Files

| File | Purpose |
|------|---------|
| `~/.ai/control/library/` | Fragment library root |
| `~/.ai/control/library/PROMPT_DESIGN.md` | Design philosophy document |
| `~/.ai/control/cc_launch_profiles.toml` | Workspace profile definitions |
| `~/.ai/smidja/nornir/cli/cc_launch/` | Launcher crate |
| `~/.ai/smidja/nornir/cli/cc_launch/CC_LAUNCH_DESIGN.md` | This document |
| `~/.ai/phoenix/coding_reference/` | Deep coding reference docs (future model for architecture/design frameworks) |
| `/tmp/cc_launch_prompt.xml` | Assembled prompt output (overwritten each launch) |

## Usage

```bash
# Launch TUI, select options, hit Enter
cc_launch

# Pass flags through to claude
cc_launch -- --model sonnet

# The TUI presents, you configure, Enter launches:
# claude --system-prompt-file /tmp/cc_launch_prompt.xml [flags]
```
