<!-- ═══════════════════════════════════════════════════════════════════════ -->
<!-- DO NOT EDIT THE PRELUDE. Before editing ANY part of this file, read     -->
<!-- /Users/johnny/.ai/CONTEXT_MANAGEMENT_SYSTEM.md — the KFM section        -->
<!-- defines what this file is for and why it is not linked from anywhere.   -->
<!--                                                                          -->
<!-- DO NOT LINK THIS FILE. Not from the CONTEXT_MAP, not from CLAUDE.md,    -->
<!-- not from the README. Its name is uninformative on purpose.              -->
<!--                                                                          -->
<!-- Entries are FAILURE SHAPE with concrete facts routed out. An entry that -->
<!-- names files and functions is a clock: it will outlive them and then     -->
<!-- assert something false to a reader who arrived here because something   -->
<!-- had already gone wrong.                                                  -->
<!-- ═══════════════════════════════════════════════════════════════════════ -->

# KFM — Nornir

Known failure modes. Each entry records something that actually went wrong here, badly
enough or often enough to be worth writing down: a mistake that kept recurring, one that
was tedious to rediscover, or one that cost hours nobody wanted to spend twice.

**This file is deliberately not linked from anything.** Not the CONTEXT_MAP, not CLAUDE.md,
not the README. It has a name that tells you nothing on purpose. That is not tidiness — it
is the point:

> A list of failure modes read *before* you have a model of the project is not a warning,
> it is a set of suggestions. Naming a mistake supplies it. And a vivid catalogue of nine
> concrete failures shadows the orientation gate it sits above — a reader absorbs the
> stories, feels informed about the project, and skips the documents that would actually
> have oriented them.
>
> Read at the right moment — when something has already gone wrong, or when you are about
> to touch a part with a known history — the same entries are worth what they cost. The
> content is not the problem. The timing is.

The mechanism for *when* a reader gets sent here is undecided. Until it is decided, nothing
points here, and that is intentional rather than an oversight.

**If you have landed here anyway:** read it as history, not as instruction. Nothing below
is a description of the tree as it stands now — check anything concrete against the code
before acting on it.

**Maintenance:** an entry retires when its vector is gone. A trap guarding code that no
longer exists is worse than no trap, because it is the rigid layer asserting something
false. This has already happened once. Nothing automatic enforces it.

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

## You Will Get These Things Wrong (salvaged)

### Data from a known source is NOT `Any`

When data comes from a defined format (YAML, JSON, TOML, our own schemas), its types ARE known — defaulting to `Any` is lazy, not honest. YAML produces `str | int | float | bool | None | dict | list`; that is the spec. Before reaching for `Any`, ask: do I actually not know this type? If it comes from a known format, define it (e.g. `type YamlLeaf = str | int | bool | None` plus recursive `YamlValue`/`YamlContainer`). Use `object` only if gleipnir says to; use `Any` only if the type is genuinely unknown.

### Bash hook rules use the Rust regex crate — no lookahead

`hook_pre_llm_bash` compiles its embedded `rules.toml` patterns with the Rust `regex` crate, which has NO lookahead/lookbehind support. Express intent with positive patterns or split one rule into several. (Example: `git stash` detection uses two rules — bare and push/save — instead of a negative lookahead.)
