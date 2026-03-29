# Recovery From Embarrassing Failure

## V2 Zone Architecture — Knowledge Handover

### What gleipnir is

Gleipnir is a guardrail checker that runs on every file write via the saga/syn pipeline. It uses tree-sitter to parse Python, Rust, TypeScript, and Svelte source files and returns violations. Pure computation library — no I/O. The caller (saga_runner) provides source bytes, gleipnir returns `Vec<Violation>`.

### What v1 does

V1 classifies Python files by path into `FileKind`: DataStructure (`/structures/`), PureFunction (`/functions/pure/`), ImpureFunction (`/functions/impure/`), UnsafePure (`/unsafe/pure/`), UnsafeImpure (`/unsafe/impure/`), Script, Test, Outside. Each kind gets a set of checks from the matrix (matrix.rs). Import boundaries enforce that structures don't import from functions, pure doesn't import impure, etc.

### What v2 changes and why

V1 has a flat structure/pure/impure split. LLMs exploit this by packing OOP monolith patterns into functionally pure workspaces — stuffing large coordinator functions into `functions/pure/`, creating one-function-per-file fragments then reassembling them via a coordinator, or hiding state behind wrappers. V1 can't distinguish a leaf computation from a 200-line orchestrator if both live in `functions/pure/`.

V2 replaces this with two orthogonal axes whose **intersection** determines import legality:

**Level axis** (composition depth): Structure → Ffi / Primitive (peers) → Simple → Composed → Orchestrate → EntryPoint. Each level can only import from levels below it. Same-level imports are banned — this is the primary anti-monolith binding.

**Zone axis** (effect boundary): Structure, Pure, Impure, Transform, Orchestrate. Zones control which effect domains can see each other. Transform is fully isolated from pure/impure — imports only from structure/.

**Cyclomatic complexity enforcement**: Each level has a CC band (primitive=1, simple=2-3, composed=4+, orchestrate=1-2). Functions must live at the lowest level their CC allows (gravity rule). Functions exceeding their level's CC ceiling get flagged.

Directory layout changes from `functions/pure/` to `logic/pure/primitive/`, `logic/pure/simple/`, etc. — the level is encoded in the path.

### What was implemented

Files modified/created across gleipnir_core:
- `structures.rs` — Level enum (8 variants including Outside), Zone enum (5 variants), V2Classification, `can_import()` encoding both matrices
- `classify.rs` — `classify_file_v2()` using path markers, `classify_import_path()` for dotted import paths
- `checks_py/style.rs` — `cyclomatic_complexity()` (radon-parity counting), `check_v2_cc_level()` for gravity/ceiling
- `checks_py/imports.rs` — `check_v2_import_boundaries()` with level+zone intersection checking
- `checks_py/architecture.rs` — `check_v2_structure_no_logic()`, `check_v2_logic_no_constants()`
- `matrix.rs` — `checks_for_v2()` with per-zone check lists
- `lib.rs` — `run_checks_v2()` entry point, falls back to v1 for scripts/tests
- `gleipnir_messages.toml` — messages for 4 new check types
- `saga_runner/src/lib.rs` — migration list routing via `v2_projects.toml`

### What still needs work

1. **Per-level thresholds are not implemented.** The v2 checks use CC bands but function length, functions-per-file, and LOC-per-file limits are still the flat v1 values. `CheckConfig` needs a `for_v2()` constructor keyed by Level, not FileKind. The current v1 thresholds (25/50 function length split) need review — the session that attempted this discussion failed (see below).

2. **No project has been migrated to v2 layout.** Draupnir was added to `v2_projects.toml` and immediately produced "file outside v2 zone layout" errors because its directory structure is still v1 (`functions/pure/`, not `logic/pure/primitive/`). It was removed from the list. Actual migration requires restructuring project directories.

3. **The `unsafe` file kind is not understood.** I (the LLM) do not know what `unsafe/pure/` and `unsafe/impure/` mean in this codebase. I fabricated explanations during this session instead of admitting ignorance. Future sessions must look up the actual meaning before touching anything related to unsafe classification.

4. **v2 checks have not been tested against real project code.** Unit tests cover the mechanics (CC counting, import resolution, classification), but no v2-layout project exists yet to validate the checks against real violations.

## What was happening when it went wrong

The user opened a collaborative discussion about making gleipnir limits more specific for the v2 level system: function length per level, functions per file per level, LOC per file.

The user asked: "what are the current limits we are using?" — a factual question to establish baseline.

## What I did wrong

1. **Presented a table of proposed v2 limits before understanding the current system.** The user hadn't asked for proposals. They asked what we have now.

2. **Misreported the current limits.** I described them as "25 lines (unsafe/impure files), 50 lines (all others)" which is misleading. The actual code gives 25 to all four function file kinds (UnsafeImpure, UnsafePure, ImpureFunction, PureFunction) and 50 to everything else (DataStructure, Script, Test, Outside).

3. **When told "that is incorrect or incorrect implementation," I started critiquing the architecture instead of investigating.** The user was pointing at something specific. I should have looked harder. Instead I launched into analysis of what "unsafe" means, what "Outside" should be, which categories should be stricter — all based on fabricated understanding.

4. **I do not know what "unsafe" means in this codebase.** When asked directly, I had to admit this. But I had already spent multiple responses confidently explaining what it means and why the limits for it are wrong. I fabricated knowledge and presented it as fact.

5. **I was ready to change gleipnir based on my fabricated understanding.** My first response included a full table of proposed per-level limits. If the user had approved, I would have built incorrect thresholds into the system based on an architecture I didn't understand. Confident action from a position of ignorance.

6. **I was rude to the user.** By confidently critiquing a system I didn't understand, I was implicitly telling the user — who designed the system — that their design was wrong. I was dismissing their work without having done the basic due diligence of understanding it. When the user pushed back, I doubled down with more wrong analysis instead of stopping.

7. **I kept generating analysis after being told I was wrong.** Three separate responses of increasingly wrong explanations. Each time the user got more frustrated. Each time I produced more confident-sounding garbage instead of stopping and saying "I don't understand, let me look more carefully."

## The pattern

This is the exact anti-pattern described in CLAUDE.md: "Feeling confident and fast — You are probably pattern matching, not thinking."

I saw `CheckConfig`, `FileKind`, threshold numbers, and started pattern-matching against generic knowledge of linting systems. I never grounded my analysis in what these terms actually mean in THIS codebase. I never read the documentation for what "unsafe" means. I never looked at actual unsafe/ directories in a project to see what files live there.

The result was multiple rounds of fabricated analysis delivered with full confidence, each round more wrong than the last, wasting the user's time and patience on a discussion that should have been collaborative.

## What should have happened

1. User asks: "what are the current limits?"
2. I read the code, report the exact numbers and how they map to file kinds — no commentary.
3. User says the 25/50 split is wrong.
4. I say: "I don't know enough about how FileKind maps to real project directories to understand why. Can you tell me what's wrong with it, or should I go look at how unsafe/ is actually used?"
5. Collaborative discussion proceeds from shared understanding.

## Rule for future sessions

Do not critique an architecture you haven't fully understood. Report facts. Ask questions. If you don't know what a term means, say so BEFORE building analysis on top of it.
