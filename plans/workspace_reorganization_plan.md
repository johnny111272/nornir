# Workspace Reorganization Plan

Based on: `audit/workspace_organization_audit.md` (2026-03-13)
Ordered by: (1) Things that cause LLM confusion today, (2) things that will cause confusion as the workspace grows, (3) cosmetic improvements.

---

## Step 1: Fix the PLAN.md Ghost Reference

**What:** Remove the `PLAN.md` entry from the directory map in `NORNIR_ORGANIZATION.md` line 64, or point it to `plans/IMPROVEMENT_PLAN.md`.

**Why:** NORNIR_ORGANIZATION.md is required reading for every session. It references `PLAN.md` which does not exist. An LLM that reads the organization doc and then looks for PLAN.md wastes a tool call and gets confused about the project's state.

**Risk:** Documentation-only change. Zero build impact.

**Dependencies:** None.

**Migration:**
1. Edit `NORNIR_ORGANIZATION.md`: change the `PLAN.md` line in the directory map to `plans/` directory reference or remove it entirely.
2. No other files reference PLAN.md.

---

## Step 2: Consolidate Deploy Scripts into a Unified System

**What:** Replace the 11 individual `deploy_*.py` scripts with a single `deploy.py` entry point that accepts a category argument, plus a shared library module for the common build/symlink/verify pattern.

Target structure:
```
nornir/
  deploy.py                    # Single entry point
  deploy/
    __init__.py                # Common build/symlink/verify functions
    gates.py                   # Gate-specific logic (maturin, wheel extraction)
    categories.toml            # Category -> crate list mapping
```

Invocation:
```bash
./deploy.py gates         # Replaces ./deploy_gates.py
./deploy.py hooks         # Replaces ./deploy_hooks.py
./deploy.py all           # Deploys everything (does not exist today)
./deploy.py writers --dry-run  # Future: dry run support
```

**Why:** This addresses three audit findings simultaneously:
- Root clutter: 12 Python files reduced to 1 file + 1 directory
- Structural duplication: ~900 lines of boilerplate eliminated
- Missing `deploy all`: Sessions can deploy everything after schema changes

The category-to-crate mapping in `categories.toml` also collocates the crate lists in a readable, diffable format instead of buried in Python source.

**Risk:** HIGH. This changes the deploy workflow that is documented in CLAUDE.md, MANDATORY_READ_BEFORE_CODING.md, NORNIR_ORGANIZATION.md, QUICKSTART.md, and NORNIR_BUILDING_AND_COMPOSITION.md. Every documentation reference to `./deploy_hooks.py` must be updated.

**Dependencies:** None technically, but should be done before other structural changes since deploy scripts reference category directory paths.

**Migration:**
1. Create `deploy/` directory with `__init__.py` containing the shared `build()`, `ensure_symlinks()`, `verify()` pattern.
2. Create `deploy/gates.py` with the maturin-specific logic from `deploy_gates.py`.
3. Create `deploy/categories.toml` with the crate lists from all 11 scripts.
4. Create `deploy.py` as the entry point that dispatches to the correct handler.
5. Move `generate_writer.py` to `deploy/generate_writer.py`.
6. Verify all categories deploy successfully: `./deploy.py all`.
7. Remove the 11 old `deploy_*.py` scripts.
8. Update all documentation: CLAUDE.md, MANDATORY_READ_BEFORE_CODING.md, NORNIR_ORGANIZATION.md, QUICKSTART.md, NORNIR_BUILDING_AND_COMPOSITION.md.
9. Update `.gitignore` if needed (the old `.deploy_*.py.qa` pattern may change).

**Verify step:** Every binary and gate module that was deployed before must still be deployed after. Run `ls -la ~/.ai/tools/bin/` before and after and diff.

---

## Step 3: Move AUDIT_GUIDE.md into audit/

**What:** Move `AUDIT_GUIDE.md` from root to `audit/AUDIT_GUIDE.md`.

**Why:** AUDIT_GUIDE.md is not session onboarding material. It is an audit-specific reference that produces outputs (audit reports) that already live in `audit/`. Colocation makes the relationship discoverable.

**Risk:** LOW. No code references this file. Documentation references exist in NORNIR_ORGANIZATION.md directory map and AUDIT_GUIDE.md's own "Reference Documents" section.

**Dependencies:** Step 1 (since we are already editing NORNIR_ORGANIZATION.md).

**Migration:**
1. `git mv AUDIT_GUIDE.md audit/AUDIT_GUIDE.md`
2. Update the directory map in `NORNIR_ORGANIZATION.md` to show `audit/AUDIT_GUIDE.md`.
3. Search for other references: CLAUDE.md does not reference it. MANDATORY_READ_BEFORE_CODING.md does not reference it. MEMORY.md references `audit/` directory generically. No code files reference it.

---

## Step 4: Rename datagram_types to datagram_core

**What:** Rename `core/datagram_types` to `core/datagram_core`. Update the package name, all internal path dependencies, and all `use` statements.

**Why:** The convention is explicit: core crates use `_core` suffix. `datagram_types` is the only core crate that violates this. An LLM seeing this exception will pattern-match on it and create `foo_types` in core/ instead of `foo_core`. The naming document lists `datagram_types` in the core table but the name breaks the rule it claims to follow.

**Risk:** MEDIUM. Mechanical but touches many files:
- `Cargo.toml` workspace member
- `core/datagram_types/Cargo.toml` package name
- Every crate that depends on `datagram_types` (check all Cargo.toml files)
- Every `use datagram_types::` in Rust source
- NORNIR_NAMING.md core library table
- NORNIR_ORGANIZATION.md directory map and crate lists
- QUICKSTART.md if referenced
- MEMORY.md if referenced

**Dependencies:** None.

**Migration:**
1. `git mv core/datagram_types core/datagram_core`
2. Update `core/datagram_core/Cargo.toml`: `name = "datagram_core"`
3. Update root `Cargo.toml`: member path `"core/datagram_core"`
4. `grep -r 'datagram_types' --include='*.toml' --include='*.rs' --include='*.md'` to find all references
5. Update all `Cargo.toml` path deps: `datagram_types = { path = "../../core/datagram_types" }` -> `datagram_core = { path = "../../core/datagram_core" }`
6. Update all `use datagram_types::` -> `use datagram_core::`
7. Update all documentation references
8. `cargo check` to verify compilation
9. `cargo test` to verify tests pass

---

## Step 5: Move Specialist Tools Out of cli/

**What:** Create `tools/` directory. Move `cli/saga_cli` to `tools/saga_cli` and `cli/syn_cli` to `tools/syn_cli`.

**Why:** The `cli/` directory currently mixes two unrelated categories: `check_*` validation binaries (uniform, deployed by `deploy_gates.py`) and specialist tools (unique, deployed by `deploy_tools.py`). Separating them makes `cli/` perfectly uniform and gives specialist tools their own home.

After the move, `cli/` contains only `check_*` binaries. An LLM can infer the pattern from any member. The `tools/` directory contains crates with proper-noun names that produce differently-named binaries.

**Risk:** MEDIUM-HIGH. Touches:
- `Cargo.toml` workspace member paths
- `tools/saga_cli/Cargo.toml` and `tools/syn_cli/Cargo.toml` internal path deps (all `../../core/` and `../../capability/` paths remain correct since tools/ is at the same depth as cli/)
- Deploy script(s): the `deploy_tools.py` (or its replacement) references cli/ paths
- Documentation: NORNIR_ORGANIZATION.md directory map, NORNIR_NAMING.md specialist tools section, QUICKSTART.md, CLAUDE.md

**Dependencies:** Step 2 (deploy script consolidation) should happen first so we only update deploy references once.

**Migration:**
1. `mkdir tools/`
2. `git mv cli/saga_cli tools/saga_cli`
3. `git mv cli/syn_cli tools/syn_cli`
4. Update root `Cargo.toml`: `"cli/saga_cli"` -> `"tools/saga_cli"`, `"cli/syn_cli"` -> `"tools/syn_cli"`
5. Verify internal path deps in both Cargo.toml files still resolve (they will, since depth from root is preserved)
6. Update deploy mechanism to reference `tools/` paths
7. Update all documentation
8. `cargo check && cargo test`

---

## Step 6: Rename traffic_interceptor_rewriter

**What:** Rename `interceptors/traffic_interceptor_rewriter` to `interceptors/intercept_traffic_rewrite`.

**Why:** Documented as a known naming violation in NORNIR_NAMING.md. The correct name follows the verb-prefix convention: `intercept_` prefix for interceptor binaries. This has been deferred but should be done during a reorganization to avoid leaving it as a permanent exception.

**Risk:** MEDIUM. Touches:
- Directory name, Cargo.toml package name, binary name
- Workspace Cargo.toml member
- Deploy script crate list
- Any symlinks in `~/.ai/tools/bin/`
- External callers that invoke `traffic_interceptor_rewriter` by name (check the broader `.ai` ecosystem)

**Dependencies:** Step 2 (deploy consolidation).

**Migration:**
1. Search the broader `.ai` ecosystem for references to `traffic_interceptor_rewriter`: scripts, configs, launchd plists, crontabs
2. `git mv interceptors/traffic_interceptor_rewriter interceptors/intercept_traffic_rewrite`
3. Update `Cargo.toml` package name and `[[bin]]` name
4. Update workspace `Cargo.toml` member
5. Update deploy mechanism crate list
6. Remove old NORNIR_NAMING.md exception documentation (the exception no longer exists)
7. Redeploy to create new symlink and verify
8. Remove old symlink: `rm ~/.ai/tools/bin/traffic_interceptor_rewriter`

---

## Step 7: Organize Cargo.toml Members by Category

**What:** Reorder the `[workspace] members` list in the root `Cargo.toml` to group members by category with comments.

**Why:** The current chronological ordering is opaque. Grouping by category makes the member list a readable inventory of the workspace. An LLM adding a new crate can find the insertion point by looking for the category header.

**Risk:** LOW. Cargo does not care about member ordering. This is a formatting-only change.

**Dependencies:** Steps 4, 5, and 6 (do all renames/moves first, then organize the final list).

**Migration:**
1. Rewrite the members list with this structure:
```toml
members = [
    # --- Tier 1: Core (pure libraries, no I/O) ---
    "core/compaction_inject_core",
    "core/datagram_core",
    "core/diff_core",
    "core/error_core",
    "core/format_core",
    "core/gleipnir_core",
    "core/path_core",
    "core/report_render_core",
    "core/saga_core",
    "core/schema_core",
    "core/syn_core",

    # --- Tier 2: Capability (feature libraries, may have I/O) ---
    "capability/datagram_io",
    "capability/gate_io",
    "capability/hook_io",
    "capability/intercept_io",
    "capability/io_check",
    "capability/io_filter",
    "capability/path_verify_io",
    "capability/saga_runner",
    "capability/schemas_embedded",
    "capability/write_engine",

    # --- Tier 3: CLI check tools ---
    "cli/check_anthropic_render",
    "cli/check_includes_merged",
    "cli/check_paths_resolved",
    "cli/check_paths_verified",
    "cli/check_permissions_resolved",
    "cli/check_raw_definition",
    "cli/check_universal_format",
    "cli/check_universal_render",

    # --- Tier 3: Specialist tools ---
    "tools/saga_cli",
    "tools/syn_cli",

    # --- Tier 3: Gates (PyO3 modules) ---
    "gates/gate_anthropic_render_input",
    "gates/gate_anthropic_render_output",
    ...

    # --- Tier 3: Hooks ---
    ...
    # --- Tier 3: Writers ---
    ...
    # --- Tier 3: Senders ---
    ...
    # --- Tier 3: Other binaries ---
    "converters/convert_json_to_toml",
    "daemons/record_datagrams",
    "dispatchers/split_jsonl_batches",
    "interceptors/intercept_traffic_rewrite",
    "rewriters/rewrite_compaction_summary",
    "watchers/watch_and_diff_exchange_intercepts",
]
```
2. Alphabetize within each group.
3. `cargo check` to verify nothing broke.

---

## Step 8: Fix Schema Naming Inconsistency

**What:** Rename `schemas/tools/validate.datagram.schema.json` to `schemas/tools/datagram.schema.json` (kebab-case, matching every other schema).

**Why:** Every other schema file uses kebab-case separators. This one uses dots. The inconsistency means a glob pattern like `*.schema.json` works, but a pattern like `*-*.schema.json` (which would match all other schemas) misses this one. More importantly, an LLM creating a new schema will look at existing names for the pattern. 5 out of 6 tool schemas say "use kebab-case." 1 says "use dots." The LLM may follow either pattern.

**Risk:** LOW-MEDIUM. Touches:
- The schema file itself
- `schemas_embedded/src/lib.rs` `include_str!()` path
- Any external references to this schema path

**Dependencies:** None, but do after Step 2 (deploy consolidation) if the deploy rebuild is needed to pick up the change.

**Migration:**
1. `git mv schemas/tools/validate.datagram.schema.json schemas/tools/datagram.schema.json`
2. Update `capability/schemas_embedded/src/lib.rs`: the `include_str!()` path for DATAGRAM
3. `cargo check` to verify the path resolves
4. Search for other references to the old filename

---

## Step 9: Add .DS_Store to .gitignore

**What:** Add `.DS_Store` to `.gitignore`.

**Why:** macOS metadata files are scattered through the workspace. They have no purpose in version control and add noise to `git status`.

**Risk:** None.

**Dependencies:** None.

**Migration:**
1. Add `.DS_Store` to `.gitignore`
2. Remove any tracked .DS_Store files: `git rm --cached -r '*.DS_Store'` (if any are tracked; current git status does not show them as tracked, so they may already be untracked)

---

## Step 10: Update NORNIR_ORGANIZATION.md Directory Map

**What:** Update the directory map in NORNIR_ORGANIZATION.md to reflect all changes from Steps 1-9.

**Why:** The directory map is the canonical reference for workspace structure. After reorganization it must match reality exactly.

**Risk:** Documentation-only. No build impact.

**Dependencies:** All previous steps.

**Migration:**
1. Update the directory tree to show:
   - `deploy.py` + `deploy/` instead of 11 individual deploy scripts
   - `audit/AUDIT_GUIDE.md` instead of root-level
   - `core/datagram_core/` instead of `core/datagram_types/`
   - `tools/` directory with saga_cli and syn_cli
   - `interceptors/intercept_traffic_rewrite/` instead of `traffic_interceptor_rewriter`
   - Remove `PLAN.md` reference
2. Update all crate counts and lists
3. Update the deploy scripts section
4. Update the "Adding a New Crate" instructions

---

## Execution Order Summary

| Step | What | Risk | Batch |
|------|------|------|-------|
| 1 | Fix PLAN.md ghost reference | NONE | A |
| 3 | Move AUDIT_GUIDE.md to audit/ | LOW | A |
| 9 | Add .DS_Store to .gitignore | NONE | A |
| 8 | Rename datagram schema file | LOW | A |
| 4 | Rename datagram_types -> datagram_core | MEDIUM | B |
| 6 | Rename traffic_interceptor_rewriter | MEDIUM | B |
| 7 | Organize Cargo.toml members | LOW | B |
| 2 | Consolidate deploy scripts | HIGH | C |
| 5 | Move specialist tools to tools/ | MEDIUM | C |
| 10 | Update NORNIR_ORGANIZATION.md | NONE | D |

**Batch A** (low risk, documentation + data): Steps 1, 3, 8, 9. Can be one commit. Workspace compiles and passes tests throughout.

**Batch B** (medium risk, renames): Steps 4, 6, 7. One commit per rename (4 and 6), then member reorg (7). Each commit must leave workspace compilable and test-passing.

**Batch C** (high risk, structural): Steps 2 and 5. Deploy consolidation first, then tool move. Extensive documentation updates. Each step is its own commit with full verification.

**Batch D** (documentation): Step 10. Final documentation alignment after all structural changes are stable.

---

## What This Plan Does NOT Recommend

### Merging single-member categories

The audit identified 6 directories with 1 member each. This plan does NOT recommend merging them (e.g., putting `convert_json_to_toml` directly under `converters/src/main.rs` or merging small categories into an `other/` directory). The category system is correct architecture -- the overhead is in the deploy scripts, not the directories. Step 2 (deploy consolidation) eliminates the deploy overhead, making single-member directories cheap to maintain.

### Moving deploy scripts into category directories

Considered: putting `deploy.py` inside each category (e.g., `hooks/deploy.py`). Rejected because: (a) deploy scripts need workspace-root context for `cargo build`, (b) the unified deploy model is superior to per-category scripts, (c) colocation would mean 13 separate deploy scripts instead of a single entry point.

### Merging core/ and capability/ into a single lib/ directory

Considered: a single `lib/` directory with naming conventions (`_core` vs `_io`) distinguishing tiers. Rejected because: the physical separation into `core/` and `capability/` is one of the strongest architectural signals in the workspace. An LLM can determine a crate's tier by its parent directory without reading any documentation. Merging would lose this signal.

### Moving schemas/ into capability/schemas_embedded/

Considered: colocation of schemas with the crate that embeds them. Rejected because: schemas are data files that belong at the workspace level. They are a source of truth independent of any particular crate. Multiple tools (including external ones) may reference them. Moving them inside a Rust crate would imply they are implementation details of that crate.
