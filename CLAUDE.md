# Nornir — Session Instructions

## Context Warning

You are working in a Rust monorepo with strict architectural conventions. After compaction or at session start, you probably feel oriented. You are almost certainly missing critical constraints that will cause you to generate structurally wrong code that compiles and passes surface inspection.

**Do not write code until you have read `NORNIR_CONVENTIONS.md`.** That document is the single source of truth for naming, organization, composition, and building rules.

If you encounter a situation where conventions are unclear: ask. Do not guess. Do not create precedent by improvising.

---

## Anti-Patterns (Accumulated From Failures)

These have all happened. Each one caused multi-session damage.

### Gleipnir violations are YOUR violations

When you edit a file and the gleipnir hook reports violations, those are your violations. All code in this repo was written by LLMs. There is no "other developer." Saying "pre-existing" or "I didn't cause this" is deflecting blame onto a previous instance of yourself.

Correct behavior: acknowledge violations as yours. If deferring, say WHY (batching, scope, risk) without distancing language. Never say "pre-existing," "not from my changes," or "I only touched X lines."

### Gleipnir checks are not "surface-level"

Never describe gleipnir checks as "surface-level," "style rules," or "lint." Every check signals something deeper: function length = accumulated responsibilities, short names = author didn't think about the reader, println in library code = misunderstanding of orchestration boundaries. Calling them surface-level is the exact dismissiveness that makes LLMs ignore gleipnir output.

### The monolith pattern

A binary starts at 50 lines. A "quick feature" adds a pure function directly to main.rs. The next session sees that function and adds another next to it. Within three sessions the binary is 900 lines, nothing is testable, nothing is reusable. **Extract pure logic to core crates immediately.** Do not accumulate.

### Manual deployment

`cargo build --release && ln -s` works in the moment. The next session won't know the binary exists, the next rebuild misses it, and other binaries sharing the same dependency are now stale. **Always use `nornir_deploy`.** See `MUST_READ_BEFORE_BUILDING.md`.

### Fixing errors one by one

When you see error lists, do NOT start fixing errors one by one. That is whack-a-mole. Find the pattern, find the functional primitive, secure the boundary. Errors disappear when the architecture is right.

---

## Stop Triggers

If you notice yourself doing any of these, STOP immediately:

- **Adding `# type: ignore` or changing types to `Any` to silence errors** — You are hiding the problem.
- **Creating a new top-level directory** without explicit user approval — The category structure is deliberate.
- **Importing from one binary crate into another** — A core crate is missing. Extract the shared logic.
- **Putting `process::exit()` in a helper function** — Return `Result`. Only `main()` exits.
- **Writing custom file I/O in a writer binary** — Use `write_engine`.
- **Reimplementing TOML rule parsing in a hook** — Use `hook_io::rules`.
- **Hardcoding `/Users/johnny/`** — Use `write_engine::ai_home()` for runtime resolution.
- **Adding `std::env` reads to a core/ crate** — Core crates are pure. Inject via parameter.
- **Running `cargo build --release` directly** — Use `nornir_deploy`. See `MUST_READ_BEFORE_BUILDING.md`.
- **Feeling confident and fast** — You are probably pattern matching, not thinking.

---

## Recovery Sources

When uncertain, read these in order:

1. **`NORNIR_CONVENTIONS.md`** — All naming, organization, composition, and building rules
2. **`MUST_READ_BEFORE_BUILDING.md`** — Why `cargo build --release` is wrong, how to use `nornir_deploy`
3. **`CONTEXT_MAP.md`** — Current crate inventory, freshness flags, "if you need X read Y" guide
4. **`audit/STRUCTURAL_AUDIT_GUIDE.md`** — Architectural invariants and what "correct" looks like
5. **`hooks/HOOK_DESIGN.md`** — Hook subsystem architecture (when working on hooks)
6. **`cli/syn_cli/SYN_DESIGN.md`** — Quality pipeline architecture (when working on syn/saga)
7. **`interceptors/INTERCEPT_DESIGN.md`** — Intercept pipeline architecture (when working on intercept/session)

---

## Key Rules

- Schemas are embedded at compile time via `include_str!()`. Changed `.schema.json` files have no effect until `nornir_deploy` runs.
- All helper functions return `Result`. Only `main()` calls `process::exit()`.
- Do not hand-write validation logic in Python. Import the gate module and call `validate()`.
- Do not bypass the gate API. Every gate returns `{"ok": bool, "data": ..., "error": ...}`. Check `ok` before using `data`.
- Run `cargo test` before committing. All tests must pass.

---

@import CONTEXT_MAP.md
