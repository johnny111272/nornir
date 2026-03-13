# Nornir Building and Composition

How to build things correctly in this workspace. Every rule exists because its opposite has happened, caused damage, and required a full audit to fix.

## The One Rule

**Compose from existing crates. Do not reimplement.**

Before writing any logic, check whether a core or capability crate already does it. If it does, import it. If it almost does, extend the existing crate. If nothing exists, create a new crate in the correct tier with a clear name — do not inline the logic in a binary.

The default LLM behavior is to write everything inline in the binary crate. This produces 900-line monoliths where pure logic, I/O, CLI parsing, and formatting are tangled together. Nothing is testable. Nothing is reusable. The next binary duplicates everything.

## Binary Structure

Every Tier 3 binary follows this pattern. No exceptions.

```rust
fn parse_args() -> Result<Config, String> { ... }

fn run(config: &Config) -> Result<String, String> { ... }

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => { eprintln!("{e}"); process::exit(2); }
    };
    match run(&config) {
        Ok(msg) => println!("{msg}"),
        Err(e) => { eprintln!("{e}"); process::exit(1); }
    }
}
```

### Rules

1. **`process::exit()` appears ONLY in `main()`.** Helper functions return `Result`. Always. This is not a style preference — it is what makes the code testable and composable. A `process::exit()` buried in a helper cannot be caught by tests, cannot be composed into a larger operation, and silently kills the process with no stack trace.

2. **`parse_args()` returns `Result`.** Bad args are an error, not a reason to terminate from inside the parser.

3. **`run()` returns `Result`.** The entire operation either succeeds or fails with a message. main decides what to do with the result.

4. **Exit codes are semantic.** 0 = success, 1 = operational failure (bad input, validation error), 2 = usage error (bad flags, missing required args).

### What NOT to do

```rust
// WRONG: process::exit in a helper
fn validate(data: &str) -> Value {
    match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("invalid JSON: {e}");
            process::exit(1);  // <-- untestable, uncomposable
        }
    }
}

// RIGHT: return Result
fn validate(data: &str) -> Result<Value, String> {
    serde_json::from_str(data)
        .map_err(|e| format!("invalid JSON: {e}"))
}
```

## Composition Patterns

### Writers (declarative on write_engine)

Writers are ~16-line binaries. They define config, call `write_engine::run()`. All validation, path safety, atomic writes, and fsync are handled by write_engine.

```rust
use schemas_embedded::MY_SCHEMA;
use write_engine::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    match write_engine::run(&WriterConfig {
        name: "append_my_thing",
        schema: &MY_SCHEMA,
        schema_source_path: "...",
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName { dir: "/path/to/output", ext: "jsonl" },
        batch_size: None,
    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => { eprintln!("{msg}"); std::process::exit(1); }
    }
}
```

**To add a new writer:**
1. Create the JSON Schema in `schemas/tools/`
2. Add the schema to `schemas_embedded/src/lib.rs`
3. Create the writer crate in `writers/` with the template above
4. Add to workspace `Cargo.toml` and `deploy_writers.py`

Do NOT write custom file I/O in writer binaries. If write_engine doesn't support what you need, extend write_engine.

### Hooks (pure decision functions on hook_io)

Hook binaries call `hook_io::run_hook(decide)` where `decide` is a pure function. All stdin parsing, JSON output formatting, datagram emission, and notification are handled by hook_io.

```rust
use hook_io::{HookDecision, HookInput};

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

fn decide(input: &HookInput) -> HookDecision {
    // Pure logic here — no I/O, no process::exit
    HookDecision::Allow
}
```

**Shared rule parsing** lives in `hook_io::rules`. If your hook uses pattern-based rules embedded in TOML, use `parse_toml_table()` and `parse_rule_array()` — do not reimplement TOML parsing in the hook binary.

### Senders (thin wrappers on datagram)

Simple senders are ~25-line binaries that construct a datagram and call `datagram_io::emit()`. The socket path, serialization, and fire-and-forget behavior are all in datagram.

**The socket path `/tmp/ai_logger.sock` lives ONLY in datagram_io.** Never hardcode it in a binary.

### QA Report Consumers (report_render_core)

Any code that groups, formats, or renders QA reports imports from `report_render_core`:

```rust
use report_render_core::{group_issues, format_output, OutputMode};
```

saga_runner generates reports. report_render_core presents them. syn, svalinn, and future consumers import report_render_core. Do NOT duplicate grouping or formatting logic in consumer binaries.

### Directory Walking (saga_runner)

saga_runner provides `walk_files()` and `find_files()` with a predicate and skip-directory list. Use these instead of writing your own recursive walk.

```rust
// Find all .py files, skipping __pycache__, node_modules, .venv, venv
let py_files = saga_runner::find_files(dir, &[], &|name| name.ends_with(".py"));

// Find .qa sidecars with extra skip directories
saga_runner::walk_files(dir, &["extra_skip"], &|name| name.ends_with(".qa"), &mut results);
```

## Dependency Rules

### Use workspace dependencies

All external crates are declared in the root `Cargo.toml` `[workspace.dependencies]`. Reference them as `{ workspace = true }` in crate-level Cargo.toml. Never pin a version in a crate's own Cargo.toml.

### Use existing core/capability crates

Before adding a dependency or writing logic, check:

| Need | Use |
|------|-----|
| JSON/YAML/TOML/TOON conversion | `format_core` |
| Schema validation | `schema_core` + `schemas_embedded` |
| Error types with educational formatting | `error_core` |
| Path field extraction from schema | `path_core` |
| Path existence checks | `path_verify_io` |
| Atomic file writes with schema validation | `write_engine` |
| QA report generation | `saga_runner` |
| QA report grouping/formatting | `report_render_core` |
| AST guardrails | `gleipnir_core` |
| Line-level diffing | `diff_core` |
| Hook stdin/stdout/decision contract | `hook_io` |
| Hook rule parsing from TOML | `hook_io::rules` |
| Gate I/O orchestration | `gate_io` |
| Datagram emission to Hlidskjalf | `datagram_io` |
| Schema constants | `schemas_embedded` |
| stdin-validate-stdout filtering | `io_filter` |
| File-arg diagnostic CLI contract | `io_check` |
| Directory walking with skip logic | `saga_runner::walk_files` |

### Tier violations

- Core crates depend ONLY on other core crates and workspace external deps
- Capability crates depend on core and other capability crates
- Binary crates depend on core and capability, NEVER on other binary crates
- No circular dependencies. Ever.

If you find yourself importing from one binary crate into another, the shared logic needs to be extracted to a core or capability crate.

## Schema-First for Data Validation

All data validation is done by JSON Schema files in `schemas/`. Schemas are embedded at compile time via `include_str!()` in `schemas_embedded`.

**To validate data:** use a schema. Do NOT write Python `if` statements or Rust `match` arms that check field types and values. That is procedural shape-checking, not schema validation.

**To add a new data shape:**
1. Create `schemas/tools/my-thing.schema.json`
2. Add `pub static MY_THING: EmbeddedValidator = ...` to `schemas_embedded/src/lib.rs`
3. Reference `MY_THING` in your binary

## Testing Requirements

### Pure logic must be testable

If a function is pure (no I/O, no env vars, no process state), it must be testable. This means:
- It takes its inputs as parameters (not from `std::env::args()`)
- It returns a value (not calls `process::exit()`)
- It has no side effects

If you need to test logic that currently reads from env/args, extract it:

```rust
// Before: untestable
fn parse_args() -> Config {
    let args: Vec<String> = std::env::args().skip(1).collect();
    parse_args_from(&args)
}

// After: parse_args_from is testable
fn parse_args_from(args: &[String]) -> Result<Config, String> { ... }
```

### Security hooks require dual-direction tests

Every detection rule must be tested in BOTH directions:
1. Malicious input IS detected (no false negatives)
2. Benign input is NOT flagged (no false positives)

A security hook with only positive tests is a ticking time bomb — you don't know what it falsely blocks.

### Test the specification, not the implementation

Tests verify that tool definition equals tool behavior. "Does `severity_rank("error")` return 2?" is a specification test. "Does the internal BTreeMap have 4 entries?" is an implementation test. Write the former.

## What Goes Wrong Without This

### The monolith pattern (most common)

1. Session writes a 400-line binary with inline JSON parsing, custom file walking, ad-hoc validation, and hand-rolled formatting
2. Next session needs similar functionality, copies the binary and modifies it
3. Third session finds a bug in the shared logic, fixes it in one copy but not the other
4. Fourth session adds a third copy with yet another variant
5. Audit reveals 3 binaries doing the same thing differently, all with bugs
6. **Full rewrite required.**

### The process::exit pattern

1. Session writes helpers that call `process::exit()` on errors
2. Tests can't catch the exits — they kill the test harness
3. No tests get written because "they don't work with this code"
4. Bugs accumulate silently because nothing verifies behavior
5. Refactoring every helper to return Result across 15 crates
6. **Multi-session audit required.**

### The reimplementation pattern

1. Session needs to walk directories for .qa files, writes its own walker
2. Another session needs to walk for .py files, writes another walker
3. A third crate needs directory walking, writes a third version
4. All three skip different directories, handle symlinks differently, have different edge cases
5. Extract to shared function, update all three callers
6. **Avoidable work.**

## Summary

| Before writing... | Check... |
|---|---|
| Any file I/O | Does write_engine or saga_runner handle this? |
| Any validation | Does a schema exist in schemas/? |
| Any formatting | Does report_render_core or format_core handle this? |
| Any directory walk | Does saga_runner::walk_files handle this? |
| Any datagram | Does datagram_io handle this? |
| Any hook logic | Does hook_io handle the contract? |
| Any process::exit | Is this in main()? If not, return Result instead. |
| Any duplicated code | Should this be in a core/capability crate? |

When in doubt: compose, don't reimplement. Extract, don't duplicate. Return Result, don't exit.
