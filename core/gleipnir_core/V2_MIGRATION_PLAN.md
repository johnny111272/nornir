# V2 Zone Architecture — Migration Plan

## Reorientation

If you're reading this after compaction or at session start: read `V2_ZONE_ARCHITECTURE.md` in this same directory first. It contains the complete architecture spec — two-axis import enforcement (level matrix + zone matrix), cyclomatic complexity bounds, zone definitions, and design rationale.

This document covers HOW to implement and roll out the architecture. The spec covers WHAT it is and WHY.

---

## Current State

Gleipnir v1 already has:
- File classification via `classify.rs` → FileKind (script, project Python, project Rust, etc.)
- Check dispatch via `matrix.rs` — different checks per FileKind
- Import zone checks (pure can't import impure) — basic version of zone enforcement
- CC measurement — exists but not tied to level enforcement
- LOC limits per function and module

The v2 architecture extends this with:
- Two new classification axes: Level (ffi/primitive/simple/composed/orchestrate) and Zone (pure/impure/transform/orchestrate)
- Level-based import checking via table lookup
- Zone-based import checking via table lookup
- Gravity and ceiling CC checks tied to level
- Structure-only checks (no functions, no non-class bindings)
- V1/V2 coexistence via saga migration list

---

## Phase 1: Codify

**Goal:** Architecture spec is complete, reviewed, stable.

**What:**
- V2_ZONE_ARCHITECTURE.md (this is done — in gleipnir_core/)
- Review against actual codebases (draupnir, regin) to verify no missed edge cases
- Confirm the two matrices generate correct legal/illegal results for all known import patterns

**Verification:** Walk through every file in draupnir and regin, manually classify (level + zone), manually check imports against both tables. Every existing legal import should pass. Every known violation should fail. Document any surprises.

**Risk:** Finding a pattern that doesn't fit the two-matrix model. If this happens, the spec needs adjustment BEFORE any code is written.

---

## Phase 2: Instrument

**Goal:** Gleipnir can detect v2 violations. Saga can route to v1 or v2 checks per project.

### 2a: Extend Classification

Add level and zone detection to `classify.rs`:

- Parse file path to extract zone (pure/impure/transform/orchestrate) and level (ffi/primitive/simple/composed)
- For v1 projects (not in migration list), classification returns current FileKind only — no change in behavior
- For v2 projects, classification returns FileKind + Level + Zone
- Path-based classification is deterministic: `logic/pure/simple/foo.py` → zone=pure, level=simple

Key files:
- `core/gleipnir_core/src/classify.rs` — file classification
- `core/gleipnir_core/src/structures.rs` — Level and Zone enums

### 2b: Add Import Checks

Encode the two matrices as static data in gleipnir_core:

- Level matrix: `static LEVEL_IMPORTS: &[(Level, &[Level])]` — for each level, which levels it can import from
- Zone matrix: `static ZONE_IMPORTS: &[(Zone, &[Zone])]` — for each zone, which zones it can import from
- Import check function: classify source file (level+zone), classify each import target (level+zone), verify both tables pass

Key files:
- `core/gleipnir_core/src/matrix.rs` — check dispatch, add v2 entries
- New check module for import analysis (level + zone tables)

### 2c: Add CC-Level Checks

- Gravity check: function CC below level minimum → violation
- Ceiling check: function CC above level maximum → violation
- Structure check: function definition in structure/ → violation
- Constant check: module-level non-class, non-function binding → violation

### 2d: Saga Migration List

- Config file mapping project paths to check versions (v1 or v2)
- `saga_runner` reads migration list, passes version to gleipnir_core
- gleipnir_core selects v1 or v2 check matrix based on version
- Location: embedded in saga_runner (like ruff.toml and pyrightconfig.json) or at a known path

Key files:
- `capability/saga_runner/src/lib.rs` — tool orchestration, version routing
- `core/gleipnir_core/src/classify.rs` — version-aware classification

### 2e: Validate Detection

- Run v2 checks against draupnir and regin WITHOUT migration (informational only)
- Verify every violation is real — no false positives
- Verify no legal patterns are flagged — no false negatives
- Count violations per category to estimate migration effort

**Verification:** `saga . --force` on both projects with v2 checks. Review every violation manually. Adjust checks if any are wrong.

---

## Phase 3: Migrate

**Goal:** One project at a time, restructure to v2 layout, opt in, fix violations until clean.

### 3a: Draupnir First (smaller project)

1. Create the v2 directory structure under draupnir
2. Move files to their correct zone/level locations:
   - `functions/pure/converters/` → `logic/transform/` (the bulk — all model-to-model conversion)
   - `functions/pure/build_cross_section.py` → `logic/transform/` (model construction)
   - `functions/pure/collect_cross_section.py` → `logic/transform/` (model traversal + collection)
   - `functions/pure/tree_walk.py` → `logic/pure/` (business logic — ref counting, tree analysis)
   - `functions/impure/pydantic_validators.py` → `logic/transform/` (coercion, remove BeforeValidator usage)
   - `functions/impure/` other files → `logic/impure/` at appropriate levels
   - Existing `structures/` stays as `structure/`
   - `main.py` / CLI entry point → entry point importing orchestrate/
3. Remove BeforeValidator usage from structure files (`fields.py` imports `ref_from_cache`)
4. Create explicit transform calls in orchestrate/ to replace implicit BeforeValidator coercion
5. Add draupnir to saga migration list
6. Run `saga . --force` → `syn .` — fix violations until clean
7. Run tests — all must pass

### 3b: Regin Second (larger, more complex)

Same process but more files:
- `functions/pure/render_regroup*.py` (4 files) → `logic/transform/`
- `functions/pure/anthropic_resolve_*.py` (7 files) → `logic/transform/`
- `functions/pure/codegen_transforms.py` → `logic/transform/`
- `functions/pure/include_bridge.py` → `logic/transform/`
- `functions/pure/path_operations.py` → `logic/pure/primitive/` (predicates, CC=1)
- `functions/pure/security_*.py` → `logic/pure/` at appropriate levels
- `functions/pure/walk_and_find.py` → `logic/pure/primitive/` (generic traversal)
- `functions/impure/pipeline.py` → split between `logic/orchestrate/` and `logic/impure/`
- Generated schema models → `structure/` (remove BeforeValidator patches from codegen_transforms)
- `branches/` (the DAG pipeline stages) → `logic/orchestrate/` or `logic/impure/composed/` depending on IO

Each migration step: move files, update imports, run saga+syn, fix violations, run tests. Small diffs. One zone at a time.

---

## Build and Deploy After Changes

After any gleipnir_core changes:

```
cargo test -p gleipnir_core
touch core/gleipnir_core/src/lib.rs  # force recompilation
nornir_deploy --build tools           # rebuild saga with new gleipnir
saga . --force --kind py              # regenerate .qa sidecars
syn .                                 # verify results
```

After saga_runner changes (migration list):

```
nornir_deploy --build tools
saga . --force
syn .
```

See `MUST_READ_BEFORE_BUILDING.md` for full deploy process. Never `cargo build --release` directly.

---

## Risk Mitigation

**False positives in new checks** — Run detection-only (Phase 2e) before any migration. Every flagged violation must be manually verified. Adjust checks before migrating.

**Breaking existing v1 projects** — V1/V2 coexistence via migration list. Projects not in the list see zero change. Migration is opt-in per project.

**Compaction amnesia** — This document and V2_ZONE_ARCHITECTURE.md are in gleipnir_core/ alongside the source code. After compaction, read both docs to reorient. The architecture spec is the source of truth. The migration plan shows where you are in the process.

**LLM resistance during migration** — The LLM will try to satisfy new checks by the cheapest path (moving functions without understanding why). Each migration step should be reviewed: is the function in the right zone AND the right level? Does it actually belong there, or was it just moved to pass the check?
