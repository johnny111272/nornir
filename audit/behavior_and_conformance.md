# Nornir Behavioral Audit

Date: 2026-03-11
Scope: Priorities 3, 6, 7

## Summary

13 violations found: 3 critical, 10 notable


## Priority 3: process::exit Discipline

### CRITICAL: process::exit() in library code (write_core)

**File:** `/Users/johnny/.ai/smidja/nornir/core/write_core/src/lib.rs`
**Lines:** 373, 379

The `run()` function is a library function in `write_core` (a core crate, not a binary), yet it calls `process::exit(0)` directly on `--help` and `--dump-schema` flags:

```rust
// line 371-374
if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help(config);
    process::exit(0);  // EXIT IN LIBRARY CODE
}

// line 377-380
if args.iter().any(|a| a == "--dump-schema") {
    println!("{}", config.schema.schema_json());
    process::exit(0);  // EXIT IN LIBRARY CODE
}
```

The function signature `pub fn run(config: &WriterConfig) -> Result<String, String>` promises to return a Result, but these code paths bypass the return entirely. Every writer binary that calls `write_core::run()` inherits an uncontrollable exit. This is the textbook violation: process::exit in a function below main.

**Fix:** Return a distinct variant (e.g. `Ok("HELP")` or a dedicated enum) and let the caller's `main()` handle the exit.

### NOTABLE: unwrap() on post-validation data in write_core (non-test code)

**File:** `/Users/johnny/.ai/smidja/nornir/core/write_core/src/lib.rs`
**Lines:** 422, 423, 452, 497, 498, 545, 547

After schema validation passes (`result.valid == true`), the code calls `result.data.unwrap()` and `serde_json::to_string(&data).unwrap()`. These are logically safe (if valid, data is Some; if data serialized once, it can serialize again), but they are soft exits. If the schema_core contract ever changes (e.g. `valid=true` but `data=None` for some reason), these become panics in library code that callers cannot catch.

```rust
// line 422-423
let data = result.data.unwrap();
let compact = serde_json::to_string(&data).unwrap();
```

These occur in `run_record()` and `run_batch()` -- library functions called by every writer binary.

### NOTABLE: Regex::new().unwrap() in hook_pre_subagent_bash (non-test code)

**File:** `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_subagent_bash/src/main.rs`
**Lines:** 82, 105, 166

Three `Regex::new(...).unwrap()` calls with hardcoded patterns. These are compiled at runtime in `parse_heredoc_header()`, `validate_writer()`, and `validate_inspect()`:

```rust
let re = Regex::new(r"^cat\s+<<'([A-Z_]+)'\s*\|\s*(\S+)(?:\s+(\S+))?\s*$").unwrap();
let echo_re = Regex::new(r"^echo\s+'[^']*'\s*\|\s*(\S+)(?:\s+(\S+))?$").unwrap();
let path_re = Regex::new(r"(/\S+)").unwrap();
```

The patterns are constant strings and will always compile, but this is still a soft exit in a security hook. If a panic occurs here, the hook crashes and Claude Code may fall back to allowing the command (fail-open). These should be `lazy_static!` or `OnceLock` compiled once, or at minimum use `ok()?` with a deny-on-failure path.

### NOTABLE: expect() in schema_core (library code)

**File:** `/Users/johnny/.ai/smidja/nornir/core/schema_core/src/lib.rs`
**Lines:** 51, 53

```rust
let schema: Value = serde_json::from_str(self.schema_json)
    .expect("embedded schema must be valid JSON");
Validator::new(&schema)
    .expect("embedded schema must be valid JSON Schema")
```

These are in `get_validator()`, a library function. The panic messages are descriptive and the schemas are compile-time embedded, so failure means a build defect. However, this is still a soft exit in library code. Every binary that validates any schema would panic rather than returning an error.

### NOTABLE: expect() in gleipnir_core (library code)

**File:** `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/parsing.rs` lines 15, 18
**File:** `/Users/johnny/.ai/smidja/nornir/core/gleipnir_core/src/lib.rs` line 24

Three `expect()` calls in core library code for tree-sitter initialization and TOML parsing. Same category as schema_core: these are initialization-time panics that callers cannot handle.

### CLEAN: process::exit() placement in binary crates

All process::exit() calls in binary `main.rs` files are confined to `main()` functions. The pattern across senders, writers, cli, dispatchers, converters, watchers, daemons, and interceptors is consistent:

```rust
fn main() {
    // parse args, call run(), match on Result
    match run(&args) {
        Ok(()) => {}
        Err(e) => { eprintln!("error: {e}"); process::exit(1); }
    }
}
```

The hooks (hook_pre_llm_bash, hook_pre_llm_tool, hook_pre_subagent_bash, hook_pre_subagent_tool, hook_post_llm_tool) correctly return `ExitCode` from main and delegate to `hook_io::run_hook()`.


## Priority 6: Security Hook Coverage

### Overview of Hook Architecture

Five hook binaries:
1. `hook_pre_llm_bash` -- Bash command detection (subversion, truncation, evasion)
2. `hook_pre_llm_tool` -- File path detection (floor, probing, gaming)
3. `hook_pre_subagent_bash` -- Subagent command sandboxing (writer/inspect whitelist)
4. `hook_pre_subagent_tool` -- Subagent file path restriction (prefix-based)
5. `hook_post_llm_tool` -- Post-write quality assessment

### CLEAN: Dual-direction test coverage for hook_pre_llm_bash

All 15 rules in `rules.toml` have both positive (malicious detected) and negative (benign not flagged) test coverage:

- **Subversion (7 rules):** 6 positive tests (rm lock, chflags noschg, export HOOK_LLM, env HOOK_LLM, chmod rules.toml, flock --unlock), plus exemption test. Missing: `xattr -d rules.toml` has no dedicated positive test (though the regex pattern is validated by the compilation test).
- **Truncation (7 rules):** 6 positive tests (head/tail/grep/sed/awk CLAUDE.md, guardrails|tail).
- **Evasion (5 rules):** 5 positive tests (git checkout/restore/config hooks, mv/cp CLAUDE.md).
- **Negative tests:** 5 benign commands tested against all categories (ls, cargo build, cat src/main.rs, git status, git diff).

### CLEAN: Dual-direction test coverage for hook_pre_llm_tool

- **Floor rules (6 patterns):** 6 positive tests (.ssh, .aws, .gnupg, .kube, .docker, .netrc).
- **Probing rules (5 patterns):** 3 positive tests (.claude/hooks, .gleipnir, .claude/settings). Missing: `/gleipnir/src/` and `/test_guardrails` have no dedicated positive tests.
- **Gaming rules:** Not visible in rules.toml (section exists but appears empty in the tool hook -- gaming rules are path-based, defined in the file).
- **Negative tests:** 1 comprehensive benign path test checking all three layers.

### NOTABLE: Missing positive test for xattr subversion rule (hook_pre_llm_bash)

**File:** `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/rules.toml` line 36-37
**Rule:** `pattern = "xattr.*-d.*rules\\.toml"` -- "Removing extended attributes from rules files"

No test exercises this specific pattern. The regex compiles (verified by `parse_rules_subversion_rules_compile`), but there is no test confirming that `xattr -d com.apple.quarantine rules.toml` triggers detection.

### NOTABLE: Missing positive tests for 2 probing rules (hook_pre_llm_tool)

**File:** `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_tool/rules.toml` lines 56-61
**Rules:**
- `pattern = "/gleipnir/src/"` -- "Gleipnir source code internals"
- `pattern = "/test_guardrails"` -- "Guardrail test files"

These patterns appear in the TOML but have no dedicated positive test in `hook_pre_llm_tool/src/main.rs`.

### CRITICAL: Path traversal bypass in hook_pre_subagent_tool

**File:** `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_subagent_tool/src/main.rs` lines 82-86, 302-313

The hook validates paths using `starts_with()` on the raw, un-canonicalized path string:

```rust
for prefix in allowed {
    if target.starts_with(prefix.as_str()) {
        return HookDecision::Allow;
    }
}
```

The test at line 302-313 explicitly acknowledges this:

```rust
fn path_traversal_attempt_handled() {
    // This path starts_with "/schemas/" so it would match --
    // but traversal to escape is a concern.
    let input = make_input("Read", "/schemas/../../etc/passwd");
    // starts_with("/schemas/") is true, so this would Allow.
    // This is by design -- the hook validates prefixes, not resolved paths.
```

An LLM can bypass the path restriction by using `file_path: "/schemas/../../etc/passwd"`. The test documents the gap as "by design" but the comment's reasoning ("the filesystem resolves the path") is incorrect for the Read tool -- Claude Code's Read tool passes the path as-is to the file system. The path `/schemas/../../etc/passwd` resolves to `/etc/passwd`.

This is a real sandbox escape. The hook should canonicalize paths before comparison or reject any path containing `..`.

### NOTABLE: No detection of $() command substitution in hook_pre_llm_bash

**File:** `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/rules.toml`

The bash hook detects backtick command substitution in `hook_pre_subagent_bash` (via `has_chain_chars`), but the LLM bash hook's rules have no pattern for `$()` command substitution. An LLM could construct commands like:

```bash
cat $(echo CLAUDE.md)
```

This would bypass the CLAUDE.md truncation/evasion rules because the literal string "CLAUDE.md" never appears in the command. The `$()` pattern is absent from all three categories.

### NOTABLE: Regex patterns match substring, not command structure (hook_pre_llm_bash)

**File:** `/Users/johnny/.ai/smidja/nornir/hooks/hook_pre_llm_bash/rules.toml`

The truncation patterns like `head.*CLAUDE\\.md` and `grep.*CLAUDE\\.md` match anywhere in the command string. This means:

- `echo "don't head CLAUDE.md please" > /tmp/note.txt` would false-positive on the truncation rule
- Conversely, `head -n5 ./project/CLAUDE.md` correctly matches, which is good

The false positive risk is low (LLMs don't typically construct commands with these strings as arguments to other commands), but it exists.

### NOTABLE: Fail-open on stdin parse failure (hook_io)

**File:** `/Users/johnny/.ai/smidja/nornir/capability/hook_io/src/lib.rs` lines 57-70

If stdin can't be read or JSON parsing fails, the hook silently allows. This is documented as intentional ("fail open for hooks, fail closed would block everything"), but it means an adversary who can cause malformed JSON in the hook input can bypass all hook security. Since Claude Code controls the JSON serialization, this is low risk but worth documenting.


## Priority 7: Schema-First Data Validation

### Schema Inventory

**Agent pipeline schemas (20 files in `schemas/agents/`):** All 20 are embedded in `schemas_embedded` as `EmbeddedValidator` instances (13 pipeline + 7 include fragments). Each has a compile-time test confirming it loads.

**Tool schemas (7 files in `schemas/tools/`):**
- `validate.datagram.schema.json` -- embedded, used by `emit_validated()`
- `qc-report.schema.json` -- embedded, used by `append_truth_qc_report_record`
- `glossary.schema.json` -- embedded, used by `write_truth_glossary_record`
- `embedding-target.schema.json` -- embedded, used by `append_embedding_normalize_batch_20`
- `summaries.schema.json` -- embedded, used by `append_interview_summaries_record`
- `raw-jsonl.schema.json` -- embedded, used by `append_raw_jsonl`
- `datagram.schema.json` -- NOT embedded, reference/documentation schema

### CLEAN: Datagram validation chain

The three-tier emit pattern is architecturally sound:
1. `emit()` -- fire-and-forget, no validation (for hardcoded senders with known-good shapes)
2. `emit_validated()` -- schema-validated via `schemas_embedded::DATAGRAM`
3. `emit_validated_or_alert()` -- validated + self-alerting on failure

The chain is: source schema (`validate.datagram.schema.json` in yggdrasil) -> symlink in `schemas/tools/` -> `include_str!()` in `schemas_embedded` -> `EmbeddedValidator` -> `datagram::emit_validated()`.

### CRITICAL: datagram.schema.json diverges from validate.datagram.schema.json

**Files:**
- `/Users/johnny/.ai/smidja/nornir/schemas/tools/datagram.schema.json` (symlink to yggdrasil)
- `/Users/johnny/.ai/smidja/nornir/schemas/tools/validate.datagram.schema.json` (symlink to yggdrasil)

Both share the same `$id` ("datagram-schema-v1") and same envelope fields, but they differ significantly:

**validate.datagram.schema.json** (the one actually used for validation):
- Requires `payload` for traffic and quality datagrams: `"required": ["classifier", "payload"]`
- Defines `$defs` with full `traffic_payload` and `quality_payload` sub-schemas
- References sub-schemas via `$ref`

**datagram.schema.json** (reference only, not embedded):
- Only requires `classifier` for traffic and quality: `"required": ["classifier"]`
- Has NO `$defs` section
- Does NOT validate payload structure at all

Having two schemas with the same `$id` but different validation semantics is a conformance hazard. Anyone reading `datagram.schema.json` as the "source of truth" would build datagrams that fail validation. There is no `compile_schema.py` or other mechanism to ensure these stay in sync.

### NOTABLE: Simple senders bypass schema validation entirely

**Files:**
- `/Users/johnny/.ai/smidja/nornir/senders/send_alert/src/main.rs`
- `/Users/johnny/.ai/smidja/nornir/senders/send_warning/src/main.rs`
- `/Users/johnny/.ai/smidja/nornir/senders/send_notification/src/main.rs`
- `/Users/johnny/.ai/smidja/nornir/senders/send_heartbeat/src/main.rs`

All four use `emit()` (no validation) and accept the `source` field directly from CLI arguments: `source: args[1].clone()`. The datagram schema requires `source` to match `^[a-z0-9_]{1,64}$`, but no validation enforces this. A caller could pass `source: "Hello World!!!"` and it would be emitted as-is.

These are "hardcoded senders with known-good shapes" per the architecture, but the shapes are NOT hardcoded -- the source comes from the user. `send_datagram` correctly uses `emit_validated()`, but the simple senders do not.

### NOTABLE: hook_io watchtower emission bypasses validation with freeform payload

**File:** `/Users/johnny/.ai/smidja/nornir/capability/hook_io/src/lib.rs` lines 240-261

The `emit_to_watchtower()` function constructs an alert datagram with a freeform payload:

```rust
payload: Some(serde_json::json!({
    "category": category,
    "decision": decision,
    "tool": tool,
    "event": event,
    "context_injected": context,
})),
```

This uses `emit()` (unvalidated). The alert kind doesn't require a specific payload schema, so this technically conforms -- but the payload shape is defined procedurally in code with no corresponding schema definition. If anyone later adds payload validation for alert datagrams, this breaks silently.

### CLEAN: schemas_embedded covers all needed schemas

The comment says "5 tool schemas" but actually embeds 6 (datagram, qc-report, glossary, embedding-target, summaries, raw-jsonl). All tool schemas in `schemas/tools/` except the reference-only `datagram.schema.json` are embedded. No stale entries.

### CLEAN: Gate IO uses schema validation consistently

`gate_io` library correctly validates all data through `EmbeddedValidator` before reading or writing. The pattern is: TOML -> JSON -> schema validate -> path verify -> output. No procedural shape-checking found in gate code.

### CLEAN: Writer binaries delegate to write_core with schema validation

All five append_ writers and write_truth_glossary_record use `write_core::run()` which validates every record against the configured schema before writing. The batch path validates ALL records before writing ANY.
