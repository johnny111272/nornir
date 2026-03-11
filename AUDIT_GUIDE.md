# Nornir Audit Guide

This document describes the architectural invariants of the nornir workspace and why they matter. It exists because LLM-generated code drifts from these invariants in ways that look correct but cause structural damage that compounds across sessions.

**Audience:** You are auditing this workspace. Your job is to find violations of these invariants. Use your own judgment to decide how to check — the invariants are described here, the detection methodology is yours.

**What this is NOT:** A linter checklist. Gleipnir already handles surface-level Python issues (print statements, None returns, naming). This guide covers architectural and structural invariants that require reading comprehension and judgment to verify.

---

## Priority 1: Pure Logic Must Not Live in Binary Crates

This is the single most damaging pattern in LLM-generated Rust code and the hardest to catch because the code works perfectly.

**The invariant:** Logic that is deterministic — takes inputs, returns outputs, no I/O, no environment access — belongs in a core library crate (`core/`), not in a binary crate (`cli/`, `hooks/`, `senders/`, etc.).

**Why it matters:** Pure logic trapped in a binary crate is invisible to the rest of the workspace. When a second binary needs the same logic, it gets reimplemented (differently, with different bugs). When a third binary needs it, you have three divergent copies. By the time you notice, extraction is a multi-session project.

**What makes this hard to catch:** The binary compiles, passes tests, does the right thing. There is no error. The damage is architectural — it's a decision that forecloses future composition. An auditor has to read the code and ask: "could anything else ever need this logic?" If yes, it shouldn't be here.

**Recent example:** syn (the QA report viewer) contained ~450 lines of pure rendering logic — grouping issues by check, severity ranking, adaptive line wrapping, line number collapsing. All pure functions, zero I/O. They lived in syn because syn was the first consumer. When svalinn needed the same rendering, the logic was trapped. Extraction to `report_render_core` was a full session of work that wouldn't have been necessary if the logic had been placed correctly from the start.

**What correct looks like:** Binary crates are thin. They handle CLI args, call into core/capability crates, and format output. The binary is orchestration; the logic is elsewhere. Writers are ~16 lines. Simple senders are ~25 lines. Even complex tools like saga keep their pure logic in saga_core and their binary is orchestration.

**Questions to ask while reading a binary:**
- If I deleted this binary, would any reusable logic disappear with it?
- Could a future tool reasonably need any of these functions?
- Are there functions here that take data in and return data out without touching the filesystem, network, or environment? Those are pure — why are they in a binary?
- Is this binary growing because new logic is being added to it, or because it's composing more capabilities? Growth from logic accumulation is the warning sign.

**Common drift patterns:**
- A binary starts small and correct, then a "quick feature" adds a pure function directly to main.rs instead of to the appropriate core crate. The next session sees that function in main.rs and adds another one next to it. Within three sessions the binary is a monolith.
- Formatting and rendering logic is especially prone to this. It's pure, it's easy to write inline, and it "only" serves this one binary — until it doesn't.
- Helper functions that transform data structures (grouping, sorting, filtering, mapping) are almost always pure logic that belongs in a core crate, even when they feel too small to extract.

---

## Priority 2: The Three-Tier Dependency Model

**The invariant:** Nornir has three tiers of crates with strict dependency rules:

- **Tier 1 (core/):** Pure library crates. Depend only on other core crates and external workspace dependencies. No I/O, no side effects.
- **Tier 2 (capability/):** Feature library crates that may perform I/O. Depend on core and other capability crates.
- **Tier 3 (binaries):** Executable crates in `cli/`, `hooks/`, `senders/`, `writers/`, `rewriters/`, `converters/`, `dispatchers/`, `watchers/`. Depend on core and capability. **Never depend on other binary crates.**

**Why it matters:** Tier violations create invisible coupling. A binary importing from another binary means their build, test, and deploy lifecycles are entangled. Shared logic between binaries signals that a core crate is missing.

**What makes this hard to catch:** Cargo doesn't enforce tiers. A binary can depend on another binary's library target and the compiler won't complain. The violation is in the `Cargo.toml` dependency declarations, not in the Rust source.

**Questions to ask:**
- Does this crate's dependency list make sense for its tier? A core crate depending on a capability crate is a tier violation even if it compiles.
- Are two binary crates sharing logic through a dependency between them? That shared logic should be in a core crate instead.
- Does a core crate perform I/O (file reads, network, environment variable access)? If so, it's either misplaced or miscategorized — it should be in capability/.
- Is there a crate in `core/` that doesn't have `_core` in its name, or a crate outside `core/` that does? The suffix and directory should agree.

**Common drift patterns:**
- A binary needs a function from another binary. Instead of extracting to a shared core crate, the developer adds a `[lib]` target to the source binary and imports from it. This "works" but creates a tier violation — binary-to-binary dependency.
- A core crate starts pure, then someone adds "just one" filesystem read because it's convenient. Now it's a capability crate wearing a core crate's name, sitting in the wrong directory.

---

## Priority 3: process::exit Discipline

**The invariant:** `process::exit()` appears ONLY in `main()`. Every other function returns `Result<T, String>` (or `Result<T, E>`). No exceptions.

**Why it matters:** `process::exit()` in a helper function makes that function untestable — it kills the test harness. It makes the function uncomposable — a caller cannot handle the error, retry, or add context. It hides control flow — reading the code, you don't see that this function can terminate the entire program.

**What makes this hard to catch:** LLMs generate `process::exit()` in helpers because training data is full of it. The code looks like proper error handling. The function "handles errors" by exiting. It works. Tests don't cover that path because they can't.

**What correct looks like:** Every function below main returns Result. Main contains exactly two match arms: one for parse_args, one for run. Exit codes are assigned there and nowhere else.

**Recent example:** During audit, process::exit calls were found buried in validation helpers, argument parsers, and I/O functions across multiple crates. Every one had to be refactored to return Result. In some cases, the exit was inside a closure passed to an iterator, making it especially hard to spot.

**Questions to ask:**
- Can every function in this binary be called from a test? If a function would kill the test harness on error, it has a hidden exit.
- Does `main()` have more than two match/if blocks that lead to exit? If so, logic that should return Result may have been flattened into main instead.
- Are there functions that return a bare value (not Result) but can encounter error conditions? That's suspicious — where does the error go?

**Common drift patterns:**
- `unwrap()` and `expect()` are soft process::exit. They panic, which in a binary is effectively an uncontrolled exit. In pure logic that should be testable, panics are just as bad as process::exit — they kill the test harness.
- A function returns `String` instead of `Result<String, String>`. It handles the error internally by printing to stderr and returning an empty string or a default value. The error is swallowed. The caller has no idea something went wrong.
- Early-return with exit gets hidden inside a chain of method calls or inside closures passed to iterators, where it's visually distant from the function signature.

---

## Priority 4: Composition Over Reimplementation

**The invariant:** Before writing any logic, check whether an existing core or capability crate already handles it. The workspace has purpose-built crates for file writing, schema validation, directory walking, QA report rendering, hook decision contracts, datagram emission, format conversion, and more.

**Why it matters:** Reimplemented logic diverges. Two directory walkers skip different directories. Two schema validators handle errors differently. Two report formatters produce different output for the same input. The bugs are subtle and the divergence is invisible until someone compares the implementations.

**What makes this hard to catch:** The reimplemented code works. It produces correct output for the inputs it was tested with. The problem only appears when edge cases differ between the two implementations, or when a fix is applied to one but not the other.

**Key crates to know about:**
- `write_core` handles all atomic file writes with schema validation
- `saga_core` handles QA report generation and directory walking
- `report_render_core` handles QA report grouping, formatting, and rendering
- `hook_io` handles the hook stdin/stdout/decision contract
- `hook_io::rules` handles TOML-based rule parsing for hooks
- `datagram` handles datagram emission (owns the socket path)
- `format_core` handles JSON/YAML/TOML/TOON conversion
- `schema_core` + `schemas_embedded` handle schema validation

If you see logic in a binary that overlaps with any of these crates, that's a violation.

**Questions to ask:**
- Is this binary doing its own file writing instead of using write_core? Its own directory walking instead of saga_core? Its own JSON-to-TOML conversion instead of format_core?
- Does this binary contain a function that looks like it belongs in one of the crates listed above? Even if the implementation differs slightly, the intent may overlap.
- Are two binaries doing similar things in different ways? That usually means both should be using a shared core function that doesn't exist yet.

**Common drift patterns:**
- "It's just a small function, not worth extracting." This is how every monolith starts. The function is small today. Tomorrow it has edge case handling. Next week it has three callers who each copied it.
- A binary hardcodes a path, a socket address, or a format string that is already defined in a core crate's constant. The values match today but will diverge when the core crate is updated and the binary isn't.
- Custom error formatting that duplicates what error_core already provides. The binary formats its own error messages with slightly different structure, making error output inconsistent across the workspace.

---

## Priority 5: Naming Encodes Architecture

**The invariant:** Names in nornir carry architectural meaning. They are not labels — they are contracts.

- Binary names use verb prefixes that encode their category: `check_`, `gate_`, `hook_`, `send_`, `append_`, `convert_`, `rewrite_`, `split_`, `watch_`
- Core crate names use `_core` suffix: `saga_core`, `write_core`, `report_render_core`, `format_core`
- Directory name = package name = binary name. Always. No aliases, no mismatches.
- The crate's category directory tells you its tier and deploy script.

**Why it matters:** An LLM encountering this workspace infers architecture from names. If a crate named `report_render` sits in `core/` without the `_core` suffix, a future session may not recognize it as a core crate. If a binary in `cli/` doesn't use a verb prefix, the next session won't know what category it belongs to or which deploy script manages it.

**What makes this hard to catch:** A wrong name compiles fine. The binary works. The damage is that every future session that reads this name gets a slightly wrong mental model. Bad names compound: bad names generate bad code that reinforces bad names.

**Questions to ask:**
- If an LLM saw only this crate's name, would it correctly infer the crate's purpose, tier, and category? If not, the name is misleading.
- Does the binary name's verb prefix match the directory it lives in? A `check_` binary should be in `cli/`, a `send_` binary in `senders/`, etc.
- Do the `Cargo.toml` package name, the `[[bin]]` name, and the directory name all agree? Any mismatch creates confusion about what the crate is actually called.
- Are there crates whose names suggest they do the same thing? Overlapping names suggest overlapping responsibilities, which suggests one of them shouldn't exist.

**Common drift patterns:**
- A crate is created with a generic name ("processor", "handler", "manager") that doesn't encode what it actually does. Every subsequent session has to open the crate and read the code to understand it.
- A crate is renamed but not all references are updated — the old name appears in comments, documentation, or error messages, creating confusion about whether two things exist or one thing has two names.
- A core crate is created without the `_core` suffix because "it's obvious from context." It's not obvious to the next LLM session, which may create a duplicate or misclassify it.

---

## Priority 6: Security Hook Coverage

**The invariant:** Hook binaries that enforce security policy must have dual-direction test coverage:

1. Known-malicious inputs ARE detected (no false negatives)
2. Known-benign inputs are NOT flagged (no false positives)

**Why it matters:** A security hook with only positive tests ("it catches bad things") is a ticking time bomb. You don't know what it falsely blocks. A developer running `cargo build` shouldn't trigger the same hook that catches `rm -rf ~/.ai/`. Both sides must be tested.

**What makes this hard to catch:** Tests that only check "bad input is blocked" look comprehensive. The test file has many cases, good coverage, all green. But every test is the same direction. The absence of false-positive tests is invisible unless you read the tests and notice what's missing.

**Hooks to pay special attention to:** The `hook_pre_llm_bash` and `hook_pre_subagent_bash` hooks enforce security policy on shell commands. Their detection rules involve regex patterns for subversion, truncation, and evasion of workspace configuration. These are the most security-critical code in the workspace.

**Questions to ask:**
- For each detection rule in a security hook, is there at least one test that confirms a benign input is NOT flagged? If every test is "bad thing is caught," the false positive coverage is zero.
- Are the regex patterns too broad? A pattern that catches `rm` will also catch `cargo rm` and `--format`. Are those false positives tested for?
- Are there categories of malicious input that no rule addresses? Think about what an adversarial LLM might try that isn't covered by existing patterns.
- Do the hook rules and the hook tests tell the same story? Rules added after tests were written may lack test coverage entirely.

**Common drift patterns:**
- New detection rules are added without corresponding tests. The rule "works" in manual testing but the test suite doesn't cover it, so future refactoring may break it silently.
- Regex patterns are tightened to fix a false positive, but the tightening introduces a false negative that isn't caught because there's no test for the original malicious input variant.
- A hook is "working fine" so nobody reads it for months. Meanwhile the threat model has evolved and the rules are stale.

---

## Priority 7: Schema-First Data Validation

**The invariant:** All structured data is validated against JSON Schema files in `schemas/`. Schemas are the source of truth. Procedural shape-checking in Rust (match arms that check field names and types) is not schema validation.

**Why it matters:** When validation logic is procedural, it drifts from the actual data format. The schema says field X is required, but the Rust code doesn't check for it. Or the Rust code checks for a field that was removed from the schema three sessions ago. The schema file and the code tell different stories.

**What correct looks like:** A JSON Schema file exists in `schemas/`. The schema is embedded via `schemas_embedded`. Validation calls the schema validator. The binary code never manually checks whether fields exist or have the right type — the schema handles that.

**Questions to ask:**
- Is there Rust code that manually checks whether a JSON field exists, what type it is, or whether it matches a set of allowed values? That's procedural shape-checking pretending to be validation.
- For each data shape that flows through the system, can you point to a `.schema.json` file that defines it? If the answer is "the validation is in the Rust code," there is no schema.
- Do the schemas in `schemas/` match what the code actually validates? A schema that says a field is required, combined with code that treats the field as optional, is a contradiction.

**Common drift patterns:**
- A binary needs to validate input "quickly" and writes a few match arms instead of calling the schema validator. The match arms work but don't cover all the constraints the schema defines. The validation is partial and nobody notices.
- A new field is added to the Rust struct but not to the schema. Or a field is removed from the schema but the Rust code still checks for it. Schema and code drift apart silently.
- Schema validation is present but error messages are generated by hand instead of from the validation result. The hand-written messages may not match the actual constraint that was violated.

---

## Priority 8: Orphaned Artifacts

**The invariant:** When files are renamed, moved, or deleted, their associated artifacts must be cleaned up.

**Why it matters:** Nornir generates `.qa` sidecar files for every Python file it scans. When a `.py` file is renamed or deleted, the old `.qa` file remains. Downstream tools (syn, svalinn) read `.qa` files at face value — an orphaned sidecar with errors appears as real violations in a file that no longer exists. This produces false positives that erode trust in the entire QA pipeline.

**This extends beyond .qa files:** Dead entries in deploy script crate lists, workspace Cargo.toml members pointing to deleted crates, symlinks in `~/.ai/tools/bin/` pointing to missing binaries, schema entries in `schemas_embedded` for removed schemas — all are orphaned artifacts that silently degrade the workspace.

**Questions to ask:**
- Does every crate listed in a deploy script still exist as a directory with source files? Does every workspace member in Cargo.toml point to a real crate?
- Are there schema files in `schemas/` that nothing references? Are there constants in `schemas_embedded` that no binary imports?
- Do the comments and documentation reference crates, files, or features that no longer exist? Stale documentation is an orphaned artifact too — it misleads future sessions.
- Has a crate been renamed but old references to the previous name survive in error messages, logging, or comments?

**Common drift patterns:**
- A crate is deleted but its entry in the workspace Cargo.toml is left behind. Cargo silently ignores the missing member in some configurations, so nobody notices until a clean build fails.
- Documentation describes a workflow involving a tool that was renamed or replaced. The next session follows the documentation, can't find the tool, and either recreates it (duplicate) or gives up (confusion).
- Test fixtures reference file paths or data shapes from a previous version of the code. The tests pass because they test the fixture, not the current code.

---

## What This Guide Does NOT Cover

**Gleipnir handles these automatically:**
- Python print() calls (should use loguru)
- Python functions returning None (should return typed values)
- Python single-letter variable names
- Python function length
- Python parameter count and naming

**MANDATORY_READ_BEFORE_CODING.md covers:**
- The compliance declaration process
- The deploy script requirement
- The pre-coding checklist

**NORNIR_BUILDING_AND_COMPOSITION.md covers:**
- The exact binary structure template
- Specific composition patterns for each crate category
- The dependency lookup table
- Concrete code examples

**This guide is the "why" and "what to look for." Those documents are the "how to do it right."**

---

## For the Auditor

You are looking for violations of the invariants above. The most valuable findings are the ones that look correct — code that compiles, passes tests, produces right answers, but violates an architectural invariant that will cause damage across future sessions.

Trust your reading of the code. If something feels like it's in the wrong place, it probably is. If a binary feels too large, it probably contains logic that should be extracted. If you see similar code in two places, one of them shouldn't exist.

The damage from architectural drift is not immediate. It's cumulative. Each small violation makes the next session's code slightly worse. This workspace has been destroyed and rebuilt from scratch multiple times because of exactly this kind of drift. Your audit prevents the next rebuild.
