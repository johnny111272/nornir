# Improvement Plan

Findings from dual-agent strict audit (2026-03-12). Both agents audited independently against AUDIT_GUIDE.md. This plan covers the intersection — findings both agents agreed on. Ordered by structural severity.

Audit reports: `audit/strict_audit_A.md`, `audit/strict_audit_B.md`

---

## 1. gleipnir_core config.rs does filesystem I/O in Tier 1

**Priority:** P2 — Three-Tier Dependency Model
**What:** `core/gleipnir_core/src/config.rs` calls `std::fs::read_to_string()` and `path.exists()`. The crate's docstring says "No I/O — caller provides source bytes." This is a lie.
**Where:** `core/gleipnir_core/src/config.rs:18-19`, `core/gleipnir_core/src/lib.rs:1` (docstring)
**Fix:** Move config loading out of gleipnir_core. The core crate should accept a `CheckConfig` (or `Option<UserConfig>`) as a parameter. The caller (saga_runner, or the binary) reads the file and passes the parsed config in. `config.rs` can stay in gleipnir_core as pure TOML parsing (accepts `&str`, returns `UserConfig`) but must not touch the filesystem. Update the docstring to be accurate.

---

## 2. record_datagrams: static mut unsoundness + process::exit in signal handler

**Priority:** P3 — Panic Discipline
**What:** `daemons/record_datagrams/src/main.rs` uses `static mut SHUTDOWN_FLAG` and `static mut SHUTDOWN_SOCKET_PATH` with 5 `unsafe` blocks. Signal handler calls `process::exit(0)` — not in main(). `static mut` is unsound: signal handlers can interrupt mid-write to these statics.
**Where:** `daemons/record_datagrams/src/main.rs:144-180`
**Fix:** Replace `static mut` with `AtomicBool` for the flag and `OnceLock<PathBuf>` (or `Mutex<Option<PathBuf>>`) for the socket path. Signal handler sets the atomic flag; main loop checks it and performs clean shutdown including `process::exit`. The signal handler itself should do nothing except set the flag.

---

## 3. syn_cli: extract pure computation to library crates

**Priority:** P1 — Pure Logic in Binary Crates
**What:** syn_cli is 1197 lines. ~200+ lines are pure computation: jq filter compilation/evaluation (`compile_filter`, `matches_filter`), three-tier filtering engine (`FilteredOutput`, `is_visible`, `apply_filters`), config parsing (`SynConfig`, `load_filter_config`). None of this touches I/O. It should be testable independently.
**Where:** `cli/syn_cli/src/main.rs:32-59` (filter engine), `65-99` (config), `306-386` (filtering)
**Fix:** Create a capability crate (likely `syn_engine` or extend `report_render_core`) that owns: filter compilation/evaluation, SynConfig parsing, three-tier filtering logic, broadcast construction. syn_cli becomes a thin binary: parse args (clap), call library, handle exit. Also remove dead `_warn_expr`/`_deny_expr` fields (audit B, P10-03).

---

## 4. JSONL-append pattern duplicated 4+ times

**Priority:** P4 — Composition Over Reimplementation
**What:** The same `OpenOptions::new().create(true).append(true).open()` + `write_all` + `flush` pattern appears in: interceptor, watcher, daemon, dispatcher. Each reimplements JSONL append independently.
**Where:** `interceptors/traffic_interceptor_rewriter/src/main.rs:127,155,184`, `watchers/watch_and_diff_exchange_intercepts/src/main.rs:350-368`, `daemons/record_datagrams/src/main.rs:89-116`
**Fix:** Add `append_jsonl_line(path: &Path, line: &str) -> Result<(), String>` to `write_engine`. It handles open, write, flush, sync. All 4 callers delegate. This is the append counterpart to `write_file_atomic`.

---

## 5. hook_pre_llm_tool decide() has no integration test

**Priority:** P6 — Security Hook Coverage
**What:** The core `decide()` function in hook_pre_llm_tool is never called in tests. Tests verify sub-components (rule parsing, pattern matching) but not the full decision path. `decide()` calls `parse_config()` which reads CLI args, making it untestable as-is.
**Where:** `hooks/hook_pre_llm_tool/src/main.rs`
**Fix:** Same pattern as hook_pre_subagent_tool: extract a `decide_with_config(input, config)` pure function that takes already-parsed config. `decide()` becomes: parse config, call `decide_with_config`. Tests call `decide_with_config` directly with constructed `HookInput` payloads. Add both-direction tests: floor rules block, probing rules block, gaming rules block, benign paths allow.

---

## 6. no_println exempts entire main.rs instead of fn main()

**Priority:** P9 — Gleipnir Check Accuracy
**What:** `is_binary_main()` checks if file path ends with `main.rs`. When true, ALL `println!` in the file is exempted — including helper functions like `parse_config()` or `run()` that should use `eprintln!`. Only `fn main()` and output-named functions should be exempt.
**Where:** `core/gleipnir_core/src/checks_rs/prohibited.rs:196-198,230,249`
**Fix:** Replace file-level exemption with function-level. When in a `main.rs` file, check if the println is inside `fn main()` specifically (walk up the AST to find the enclosing `function_item`, check if its name is `main`). Output-named function exemption (`print_*`, `emit_*`, `display_*`) already works correctly and stays. Update tests: `println_in_main_rs_ok` should specifically test inside `fn main()`, and add a test that `println!` in a helper function within main.rs IS caught.

---

## 7. io_check has zero tests (8 consumers)

**Priority:** P8 — Stale Tests
**What:** `capability/io_check/src/lib.rs` has no `#[cfg(test)]` module. This crate provides the `run_check()` entry point used by all 8 `check_*` CLI binaries. A contract change propagates silently.
**Where:** `capability/io_check/src/lib.rs`
**Fix:** Add tests for `run_check()` with: valid input (exits ok), invalid input (exits with diagnostic), missing file (exits with error), stdin mode. The function writes to stdout so tests may need to capture output or the function needs a minor refactor to return structured results that main() formats. Study the current contract before deciding approach.

---

## 8. Hook inputs and syn config parsed without schema

**Priority:** P7 — Schema-First Data Validation
**What:** `HookInput.tool_input` is `serde_json::Value` — untyped blob. All hooks manually `.get("file_path").and_then(|v| v.as_str())`. syn config (`.syn/warn.toml`, `.syn/deny.toml`) is parsed with raw TOML field extraction.
**Where:** `capability/hook_io/src/lib.rs:26`, `cli/syn_cli/src/main.rs:75-99`
**Fix:** For hook inputs: Claude Code defines the hook input contract. Create typed structs for PreToolUse input (tool_name, tool_input with known fields like file_path, command, content). Deserialize into the struct instead of Value. For syn config: create a schema or at minimum a typed SynFilterConfig struct with serde derives. The schema question is whether these shapes are stable enough to warrant `.schema.json` files or whether typed Rust structs are sufficient. Either way, stop doing `.get().and_then()` chains.

---

## 9. ~~datagram and path_verify lack tier-indicating names~~ DONE

Renamed `datagram` → `datagram_io`, `path_verify` → `path_verify_io`. Mechanical find-and-replace across 22 Cargo.toml + 17 Rust source files + 6 documentation files. Removed 2 stale path_verify deps from check_paths_resolved and check_raw_definition.

---

## 10. ~~Hardcoded absolute paths in writer binaries~~ DONE

Changed `OutputPath` fields from `&'static str` to `PathBuf`, `schema_source_path` from `&'static str` to `String`. Added `write_engine::ai_home()` to resolve `$HOME/.ai` at runtime. All 5 writers now use `ai_home().join(...)` for relative paths. Zero `/Users/johnny` strings remain in writer source.
