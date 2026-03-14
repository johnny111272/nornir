# Improvement Plan

Findings from dual-agent strict audit (2026-03-12). Both agents audited independently against AUDIT_GUIDE.md. This plan covers the intersection — findings both agents agreed on. Ordered by structural severity.

Audit reports: `audit/strict_audit_A.md`, `audit/strict_audit_B.md`

---

## 1. ~~gleipnir_core config.rs does filesystem I/O in Tier 1~~ DONE

Removed `config.rs` entirely. Gleipnir configuration is now handled externally — callers parse the config and pass it in.

---

## 2. ~~record_datagrams: static mut unsoundness + process::exit in signal handler~~ DONE

Replaced `static mut` with `AtomicBool` + `OnceLock`. Signal handler sets atomic flag only; main loop performs clean shutdown.

---

## 3. ~~syn_cli: extract pure computation to library crates~~ DONE

Created `core/syn_core/` with jq filter compilation, three-tier filtering engine, and SynConfig types. syn_cli is now thin orchestration (~457 lines). syn_core has 39 tests.

---

## 4. ~~JSONL-append pattern duplicated 4+ times~~ DONE

Added `write_engine::append_line_fsync`. Interceptor, daemon, and intercept_io now delegate to it.

---

## 5. ~~hook_pre_llm_tool decide() has no integration test~~ DONE

Extracted `decide_inner` pattern. Tests call `decide_inner` with constructed Config+Rules. Both-direction coverage for floor, probing, gaming, and benign paths.

---

## 6. ~~no_println exempts entire main.rs instead of fn main()~~ DONE

Replaced file-level exemption with function-level AST check (`in_main_function`). println in helper functions within main.rs is now caught. Tests verify both directions.

---

## 7. ~~io_check has zero tests (8 consumers)~~ DONE

Added 13+ tests for arg parsing, validation, error paths, and stdin mode.

---

## 8. ~~Hook inputs and syn config parsed without schema~~ DONE

Added accessor methods to HookInput/PostHookInput (`target_path()`, `command()`). All hooks now use accessors instead of raw `.get().and_then()` chains. Syn config uses typed `SynFilterToml` struct with serde derives instead of raw TOML extraction. 11 tests for accessor methods.

---

## 9. ~~datagram and path_verify lack tier-indicating names~~ DONE

Renamed `datagram` → `datagram_io`, `path_verify` → `path_verify_io`. Mechanical find-and-replace across 22 Cargo.toml + 17 Rust source files + 6 documentation files. Removed 2 stale path_verify deps from check_paths_resolved and check_raw_definition.

---

## 10. ~~Hardcoded absolute paths in writer binaries~~ DONE

Changed `OutputPath` fields from `&'static str` to `PathBuf`, `schema_source_path` from `&'static str` to `String`. Added `write_engine::ai_home()` to resolve `$HOME/.ai` at runtime. All 5 writers now use `ai_home().join(...)` for relative paths. Zero `/Users/johnny` strings remain in writer source.
