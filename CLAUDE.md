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

<!-- Salvaged memory residue routed into nornir. Additive only. -->

## You Will Get These Things Wrong (salvaged)

### Data from a known source is NOT `Any`

When data comes from a defined format (YAML, JSON, TOML, our own schemas), its types ARE known — defaulting to `Any` is lazy, not honest. YAML produces `str | int | float | bool | None | dict | list`; that is the spec. Before reaching for `Any`, ask: do I actually not know this type? If it comes from a known format, define it (e.g. `type YamlLeaf = str | int | bool | None` plus recursive `YamlValue`/`YamlContainer`). Use `object` only if gleipnir says to; use `Any` only if the type is genuinely unknown.

### Bash hook rules use the Rust regex crate — no lookahead

`hook_pre_llm_bash` compiles its embedded `rules.toml` patterns with the Rust `regex` crate, which has NO lookahead/lookbehind support. Express intent with positive patterns or split one rule into several. (Example: `git stash` detection uses two rules — bare and push/save — instead of a negative lookahead.)

## Recovery Sources (salvaged)

- **announce (TTS subsystem)** — operational reference (backends, control directory, workspace registry, lock resolution, config priority, ElevenLabs specifics) salvaged to `cli/announce/ANNOUNCE_NOTES.md`.

## Project Notes (salvaged)

### Group module contents by action, not by consumer

Functions belong together by WHAT they do, never by WHO consumes them. Grouping by consumer (e.g. `converters_field` serves field-tier, `converters_simple` serves simple-tier) scatters one action across modules and mixes actions within a module. Group by action (`schema_build`, `field_annotate`, `conditional_build`, `zone_collect`). When organizing, ask "what does this function DO?" — if two functions both build JSON Schema nodes they belong together even if one serves atoms and the other serves sections. A module mixing building + metadata + dispatching must be split by action.

### Hook response builders (hook_io::response)

`capability/hook_io/src/response.rs` defines one response type per Claude Code hook event; each type exposes only the methods valid for that event, so invalid combinations are compile-time errors, not runtime checks. Our terms map to the Anthropic wire format per event: Allow → `permissionDecision:"allow"` (PreToolUse) / omit decision (PostToolUse/Stop) / `behavior:"allow"` (PermissionRequest); Deny → `permissionDecision:"deny"` / `decision:"block"` / `behavior:"deny"`; context → `hookSpecificOutput.additionalContext`; reason → `permissionDecisionReason` (Pre) / top-level `reason` (Post/Stop) / `decision.message` (PermissionRequest). `Ask` is PreToolUse-only. Universal fields on every type (via `WithUniversal`): `suppress_output()`, `system_message(msg)`, `stop_session(reason)`. Usage: `PreToolUseResponse::allow().with_reason(...).with_context(...)`, `PreToolUseResponse::deny(reason)`, `PostToolUseResponse::allow().with_context(...)`.

### Workspace identity derivation

`datagram::workspace_from_path(path)` derives a `@{relative}` workspace name from the `~/.ai/` base — e.g. `/Users/johnny/.ai/smidja/nornir` → `@smidja/nornir`. Paths outside `~/.ai/`, or no path, resolve to `@`. This is the universal convention for any tool that operates on directories (syn, kvasir, etc.).

### Why specialist tools carry the `_cli` package suffix

`saga` and `syn` are proper-noun binaries with no verb prefix. Their package/directory names are `saga_cli`/`syn_cli` — the `_cli` suffix on `syn` is mandatory because the bare name `syn` collides with the well-known Rust `syn` crate. Binary name stays `syn`.

### Gleipnir: fuzzy-returns policy and shared ownership detection

Gleipnir violation messages embed a FUZZY RETURNS POLICY: never reveal the numeric thresholds that triggered a check (revealing them invites gaming). In the Rust checks, `clone_is_ownership_transfer()` (in `checks_rs/prohibited.rs`) is a single AST parent-walking function shared by BOTH `no_clone_spam` and `no_string_abuse` — it decides whether a `.clone()`/`.to_string()` flows into an ownership-requiring context (struct field, function arg, return, match arm, etc.). Changing ownership-detection logic affects both checks at once.

---

@import CONTEXT_MAP.md
