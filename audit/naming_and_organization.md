# Nornir Naming and Organization Audit

**Date:** 2026-03-08
**Auditor:** Automated (Claude Opus 4.6)
**Specifications used:** NORNIR_NAMING.md, NORNIR_ORGANIZATION.md, MANDATORY_READ_BEFORE_CODING.md

---

## Summary

| Metric | Count |
|--------|-------|
| Total crates audited | 80 |
| PASS | 74 |
| FAIL | 6 |
| Violations found | 8 |

### Violation Summary

| # | Crate | Category | Violation |
|---|-------|----------|-----------|
| V1 | `intercept_io` | capability | Not listed in NORNIR_ORGANIZATION.md or NORNIR_NAMING.md; undocumented capability crate |
| V2 | `intercept_io` | capability | Uses `cdylib` crate-type (PyO3), unusual for a Tier 2 capability crate |
| V3 | `qa_report` | cli | Missing verb prefix: should be `check_qa_report` or similar; `qa_report` does not follow any recognized naming convention |
| V4 | `qa_core` | core | Marked as legacy in NORNIR_ORGANIZATION.md but still in workspace members |
| V5 | `qa_report` | cli | Marked as legacy in NORNIR_ORGANIZATION.md but still in workspace members |
| V6 | `split_jsonl_batches` | dispatchers | Missing `[[bin]]` section in Cargo.toml |
| V7 | `split_jsonl_batches` | dispatchers | No deploy script exists for dispatchers category |
| V8 | `qa_report` | cli | Not covered by any deploy script (not in deploy_gates.py CLI_CRATES, not in deploy_tools.py) |

---

## Audit by Category

### core/ -- Tier 1 Pure Libraries

Convention: `{domain}_core` name, `rlib` crate-type, no I/O, directory name = package name.

| Crate | Dir Match | Name Convention | Crate Type | Deps Compliant | Result |
|-------|-----------|-----------------|------------|----------------|--------|
| `error_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS | **PASS** |
| `format_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS, internal dep `error_core` via path PASS | **PASS** |
| `schema_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS, internal dep `error_core` via path PASS | **PASS** |
| `path_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS, internal dep `error_core` via path PASS | **PASS** |
| `write_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS, internal deps `error_core`, `schema_core` via path PASS | **PASS** |
| `saga_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS, internal dep `gleipnir_core` via path PASS | **PASS** |
| `gleipnir_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS | **PASS** |
| `diff_core` | PASS | PASS (`_core` suffix) | `rlib` PASS, explicit `[lib] name` matches | no deps PASS | **PASS** |
| `qa_core` | PASS | PASS (`_core` suffix) | `rlib` PASS | workspace deps PASS | **FAIL** |

**qa_core violations:**
- V4: Listed as "(legacy, being phased out)" in NORNIR_ORGANIZATION.md but still present as a workspace member. This is a documentation/cleanup discrepancy -- the crate itself follows naming conventions but its continued presence contradicts the documented status.

---

### capability/ -- Tier 2 Feature Libraries

Convention: No mandatory suffix, names describe capability, `rlib` crate-type, directory name = package name.

| Crate | Dir Match | Name Convention | Crate Type | Deps Compliant | Result |
|-------|-----------|-----------------|------------|----------------|--------|
| `schemas_embedded` | PASS | PASS | `rlib` PASS | internal dep `schema_core` via path PASS | **PASS** |
| `path_verify` | PASS | PASS | `rlib` PASS | internal deps via path PASS | **PASS** |
| `io_filter` | PASS | PASS | `rlib` PASS | internal dep `error_core` via path PASS | **PASS** |
| `io_check` | PASS | PASS | `rlib` PASS | workspace deps PASS, internal dep via path PASS | **PASS** |
| `gate_io` | PASS | PASS | `rlib` PASS | internal deps via path PASS | **PASS** |
| `hook_io` | PASS | PASS | `rlib` PASS | workspace deps PASS, internal dep `socket_emit` via path PASS | **PASS** |
| `socket_emit` | PASS | PASS | `rlib` PASS | workspace deps PASS | **PASS** |
| `intercept_io` | PASS | PASS (descriptive name) | `cdylib` + `rlib` **QUESTIONABLE** | workspace deps PASS, internal dep via path PASS | **FAIL** |

**intercept_io violations:**
- V1: Not listed in NORNIR_ORGANIZATION.md's capability directory map or NORNIR_NAMING.md's capability table. It is an undocumented addition to the workspace.
- V2: Uses `crate-type = ["cdylib", "rlib"]` with a PyO3 dependency. Capability crates in the spec are `rlib` only. The `cdylib` + PyO3 pattern is reserved for `gates/` crates. A capability crate should not produce a Python extension module -- it should be a pure Rust library consumed by other crates.

---

### gates/ -- Tier 3 PyO3 Pipeline Gate Modules

Convention: `gate_{stage}_{direction}` name, `cdylib` + `rlib` crate-type, directory name = package name = lib name.

All 33 gate crates checked. Every one follows the pattern exactly.

| Crate | Dir Match | Name Convention | Crate Type | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|------------|----------------|-----------------|--------|
| `gate_raw_definition_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_paths_resolved_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_paths_verified` | PASS | PASS (passthrough, no direction suffix per spec) | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_paths_verified_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_guardrails_reduced_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_guardrails_reduced_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_success_reduced_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_success_reduced_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_criteria_merged_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_criteria_merged_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_instructions_reduced_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_instructions_reduced_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_examples_reduced_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_examples_reduced_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_execution_merged_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_execution_merged_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_includes_merged_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_includes_merged_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_permissions_resolved_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_permissions_resolved_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_universal_format_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_universal_format_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_universal_render_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_universal_render_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_anthropic_render_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_anthropic_render_output` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_success_criteria_input` | PASS | PASS (include fragment gate per spec) | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_failure_criteria_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_execution_instructions_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_example_entries_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_example_group_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_guardrails_constraints_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |
| `gate_include_guardrails_anti_patterns_input` | PASS | PASS | `cdylib`+`rlib` PASS | PASS | deploy_gates.py PASS | **PASS** |

---

### cli/ -- Tier 3 CLI Validation + Specialist Tools

Convention: `check_*` for validators, specialist tools use proper nouns (saga, syn) with `{name}_cli` package producing `{name}` binary.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `check_raw_definition` | PASS | PASS (`check_` prefix) | implicit (no `[[bin]]`) PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_paths_resolved` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_paths_verified` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_includes_merged` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_permissions_resolved` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_universal_format` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_universal_render` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `check_anthropic_render` | PASS | PASS | implicit PASS | PASS | deploy_gates.py PASS | **PASS** |
| `saga_cli` (dir: `saga`) | PASS | PASS (specialist tool exception) | `[[bin]] name = "saga"` PASS | workspace deps PASS | deploy_tools.py PASS | **PASS** |
| `syn_cli` (dir: `syn`) | PASS | PASS (specialist tool exception) | `[[bin]] name = "syn"` PASS | workspace deps PASS | deploy_tools.py PASS | **PASS** |
| `qa_report` | PASS (dir match) | **FAIL** (no verb prefix) | `[[bin]] name = "qa_report"` | dep `qa_core` via path PASS | **NOT IN ANY DEPLOY SCRIPT** | **FAIL** |

**qa_report violations:**
- V3: Binary name `qa_report` does not follow verb-prefix convention. Per NORNIR_NAMING.md, CLI binaries in `cli/` should use the `check_` prefix. It is not a specialist tool (those are proper nouns like saga/syn). Should be renamed to something like `check_qa_report` or removed if legacy.
- V5: Listed as "(legacy, being phased out)" in NORNIR_ORGANIZATION.md but still in workspace members.
- V8: Not covered by any deploy script. Not in deploy_gates.py's CLI_CRATES list, not in deploy_tools.py.

---

### writers/ -- Tier 3 Schema-Validated Output Writers

Convention: `append_*` or `write_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `append_truth_qc_report_record` | PASS | PASS (`append_` prefix) | `[[bin]]` matches PASS | PASS | deploy_writers.py PASS | **PASS** |
| `write_truth_glossary_record` | PASS | PASS (`write_` prefix) | `[[bin]]` matches PASS | PASS | deploy_writers.py PASS | **PASS** |
| `append_embedding_normalize_batch_20` | PASS | PASS (`append_` prefix) | `[[bin]]` matches PASS | PASS | deploy_writers.py PASS | **PASS** |
| `append_interview_summaries_record` | PASS | PASS (`append_` prefix) | `[[bin]]` matches PASS | PASS | deploy_writers.py PASS | **PASS** |
| `append_raw_jsonl` | PASS | PASS (`append_` prefix) | `[[bin]]` matches PASS | workspace dep `serde_json` PASS | deploy_writers.py PASS | **PASS** |

---

### hooks/ -- Tier 3 LLM Security Interceptors

Convention: `hook_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `hook_pre_llm_tool` | PASS | PASS (`hook_` prefix) | `[[bin]]` matches PASS | PASS | deploy_hooks.py PASS | **PASS** |
| `hook_pre_llm_bash` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_hooks.py PASS | **PASS** |
| `hook_post_llm_tool` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_hooks.py PASS | **PASS** |
| `hook_pre_subagent_tool` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_hooks.py PASS | **PASS** |
| `hook_pre_subagent_bash` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_hooks.py PASS | **PASS** |

---

### senders/ -- Tier 3 Datagram Emitters

Convention: `send_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `send_heartbeat` | PASS | PASS (`send_` prefix) | `[[bin]]` matches PASS | PASS | deploy_senders.py PASS | **PASS** |
| `send_notification` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_senders.py PASS | **PASS** |
| `send_warning` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_senders.py PASS | **PASS** |
| `send_alert` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_senders.py PASS | **PASS** |
| `send_datagram` | PASS | PASS | `[[bin]]` matches PASS | PASS | deploy_senders.py PASS | **PASS** |

---

### converters/ -- Tier 3 Format Conversion Binaries

Convention: `convert_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `convert_json_to_toml` | PASS | PASS (`convert_` prefix) | `[[bin]]` matches PASS | PASS | deploy_converters.py PASS | **PASS** |

---

### rewriters/ -- Tier 3 JSON Request Rewriters

Convention: `rewrite_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `rewrite_compaction_summary` | PASS | PASS (`rewrite_` prefix) | `[[bin]]` matches PASS | workspace dep PASS | deploy_rewriters.py PASS | **PASS** |

---

### watchers/ -- Tier 3 File/Event Watcher Binaries

Convention: `watch_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `watch_and_diff_exchange_intercepts` | PASS | PASS (`watch_` prefix) | `[[bin]]` matches PASS | PASS | deploy_watchers.py PASS | **PASS** |

---

### dispatchers/ -- Tier 3 Batch Processing

Convention: `split_*` prefix, directory name = package name = binary name.

| Crate | Dir Match | Name Convention | Bin Name Match | Deps Compliant | Deploy Coverage | Result |
|-------|-----------|-----------------|----------------|----------------|-----------------|--------|
| `split_jsonl_batches` | PASS | PASS (`split_` prefix) | **FAIL** (no `[[bin]]` section) | workspace deps PASS | **FAIL** (no deploy_dispatchers.py) | **FAIL** |

**split_jsonl_batches violations:**
- V6: Missing `[[bin]]` section in Cargo.toml. The crate relies on implicit binary detection from `src/main.rs`. While Cargo will still build the binary, this violates the template in NORNIR_ORGANIZATION.md which requires an explicit `[[bin]] name` matching the directory name.
- V7: No deploy script exists for the dispatchers category. MANDATORY_READ_BEFORE_CODING.md states "Every binary must be added to the appropriate deploy script." NORNIR_ORGANIZATION.md lists deploy scripts for gates, hooks, writers, rewriters, and tools -- but dispatchers have no deploy script. A `deploy_dispatchers.py` should be created.

---

## Deploy Script Coverage Cross-Reference

| Deploy Script | Crates Listed | Crates in Workspace | Coverage |
|---------------|---------------|---------------------|----------|
| `deploy_gates.py` | 8 CLI + 33 gates = 41 | 8 CLI + 33 gates = 41 | COMPLETE |
| `deploy_hooks.py` | 5 hooks | 5 hooks | COMPLETE |
| `deploy_writers.py` | 5 writers | 5 writers | COMPLETE |
| `deploy_senders.py` | 5 senders | 5 senders | COMPLETE |
| `deploy_converters.py` | 1 converter | 1 converter | COMPLETE |
| `deploy_watchers.py` | 1 watcher | 1 watcher | COMPLETE |
| `deploy_rewriters.py` | 1 rewriter | 1 rewriter | COMPLETE |
| `deploy_tools.py` | 2 tools (saga, syn) | 2 tools | COMPLETE |
| (missing) `deploy_dispatchers.py` | N/A | 1 dispatcher | **MISSING** |

**Uncovered binaries:**
- `split_jsonl_batches` -- no deploy script for dispatchers category
- `qa_report` -- not in any deploy script (legacy, should be removed or added)

---

## Documentation vs. Reality Discrepancies

1. **NORNIR_ORGANIZATION.md lists `deploy_rewriters.py`** -- this file exists and is correct. PASS.
2. **NORNIR_ORGANIZATION.md does NOT list `deploy_senders.py`, `deploy_converters.py`, `deploy_watchers.py`** -- these deploy scripts exist on disk but are not documented in the "Deploy Scripts" table of NORNIR_ORGANIZATION.md. The ORGANIZATION doc only lists: `deploy_gates.py`, `deploy_hooks.py`, `deploy_writers.py`, `deploy_rewriters.py`, `deploy_tools.py`. Three deploy scripts are undocumented.
3. **`intercept_io` capability crate** is in the workspace Cargo.toml members list but not documented in either NORNIR_NAMING.md or NORNIR_ORGANIZATION.md.
4. **NORNIR_ORGANIZATION.md lists `rewriters/` directory** and `deploy_rewriters.py` -- both exist. PASS.

---

## Recommendations

### Priority 1: Fix Violations

1. **Remove or rename `qa_report`:** Either remove `qa_report` and `qa_core` from workspace members (they are documented as legacy), or rename `qa_report` to `check_qa_report` if it is being kept.
2. **Add `[[bin]]` section to `split_jsonl_batches/Cargo.toml`:** Add explicit `[[bin]] name = "split_jsonl_batches"` and `path = "src/main.rs"`.
3. **Create `deploy_dispatchers.py`:** Follow the pattern of other deploy scripts. Add `split_jsonl_batches` to its crate list.
4. **Resolve `intercept_io` status:** Either document it in NORNIR_NAMING.md and NORNIR_ORGANIZATION.md, or move it to `gates/` if it is meant to be a PyO3 module, or remove the `cdylib` crate-type if it should remain a capability crate.

### Priority 2: Documentation Updates

5. **Update NORNIR_ORGANIZATION.md deploy script table** to include `deploy_senders.py`, `deploy_converters.py`, and `deploy_watchers.py`.
6. **Add `intercept_io` to capability crate table** in NORNIR_NAMING.md if it is a legitimate capability crate.
