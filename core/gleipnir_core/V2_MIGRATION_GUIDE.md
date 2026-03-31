# V1 → V2 Zone Architecture Migration Guide

Practical guide for migrating a Python project from v1 (functions/pure, functions/impure, structures/) to v2 zone architecture (logic/{pure,impure,transform,orchestrate}, structure/).

Based on the regin migration (41→50 logic files, 12→9 structure files, quality 45→82).

---

## Prerequisites

Before starting:
1. Project must already work end-to-end (pipeline passes, tests pass)
2. Add project to `saga_runner/v2_projects.toml` so saga routes it through v2 checks
3. Run `saga . --force` + `syn .` to get a v1 baseline violation count

---

## Phase 1: Zone Assignment (get files into the right directories)

This is the biggest change. Every file moves from its v1 location to a v2 zone.

### The zone decision tree

For each file, ask:
1. **Does it define data types only (Pydantic models, enums, TypedDicts)?** → `structure/`
2. **Does it do I/O (disk, network, subprocess, gates)?** → `logic/impure/`
3. **Does it transform model A → model B (shape change)?** → `logic/transform/`
4. **Does it compute without I/O or shape change (validate, derive, resolve)?** → `logic/pure/`
5. **Does it wire multiple zones together (pipeline orchestration)?** → `logic/orchestrate/`

### Common v1 → v2 mappings

| v1 location | v2 zone | Notes |
|---|---|---|
| `structures/` | `structure/gen/schema/` | Generated Pydantic models (don't touch) |
| `structures/*.py` (hand-written) | `structure/model/` | FoundItem, BlockingViolation, etc. |
| `functions/pure/` | `logic/pure/` or `logic/transform/` | See "pure vs transform" below |
| `functions/impure/` | `logic/impure/` | Gates, registry, file loading |
| `branches/` | `logic/orchestrate/` | L0-L8 pipeline stages |
| `functions/impure/pipeline.py` | `logic/entry_point.py` | Top-level orchestration |

### Pure vs Transform — the tricky call

A v1 `functions/pure/` file might be pure computation OR a transform. The test:

- **If it takes typed model A and returns typed model B** (different schema) → `transform/`
- **If it takes data and returns derived/validated data** (same domain) → `pure/`

Examples from regin:
- `security_blocking.py` (validates security invariants, returns violations) → `pure/security_validate/`
- `render_regroup.py` (reshapes 6 domain sections → 12 consumption sections) → `transform/section_regroup/`
- `anthropic_resolve_compose.py` (universal format → anthropic format) → `transform/vendor_compose/`
- `path_operations.py` (intersects paths with capabilities) → `pure/path_operations/`

### Naming: action-based, not owner-based

v1 names by pipeline stage: `anthropic_resolve_grants.py`, `render_regroup_content.py`
v2 names by action: `grant_expand/`, `section_regroup/`

The test: if an LLM saw ONLY the directory name, would it know what the code does?
- `anthropic_resolve_compose` → no (what does "resolve compose" mean?)
- `vendor_compose` → yes (composes vendor-specific output)

### Merging related files

v1 often has multiple files for the same action split by pipeline stage:
- `security_explicit.py` + `security_implicit.py` + `security_output_tool.py` → merge into `grant_derive/`
- `render_regroup.py` + `render_regroup_content.py` + `render_regroup_sections.py` → merge into `section_regroup/`

Rule: if 2-3 files do the same kind of work on the same domain, they belong in one module directory.

### Execution

1. Create all new directories with `__init__.py`
2. Move files one module at a time
3. Update imports in the moved file
4. Update imports in all consumers (orchestrate/ files)
5. Test E2E after each module move
6. Delete empty old directories last

Do NOT try to move everything at once. One module, test, next module, test.

---

## Phase 2: CC Level Decomposition (split files into levels)

After zones are correct, each module directory gets split into CC levels.

### The level hierarchy

```
primitive.py   CC=1, no cross-module deps, no branching
simple.py      CC=1-3, may import primitive/ffi from same module
dispatch.py    CC=1-2, thin routing between simples
composed.py    CC=4-8, may import dispatch/simple/primitive
assembled.py   CC=1-2, thin composition of composed functions
```

Not every module needs all 5 levels. Most modules have 2-3 level files (e.g., `simple.py` + `composed.py`, or `primitive.py` + `simple.py` + `composed.py` + `assembled.py`).

### How to split

1. **Measure CC of every function** (count if/elif/for/while/except/ternary/boolean-and-or + 1)
2. **CC=1 functions with NO cross-module deps** → `primitive.py`
3. **CC=1-3 functions** (with or without deps) → `simple.py`
4. **CC=4+ functions** → `composed.py`
5. **If composed.py needs to wire multiple composed functions** → extract wiring to `assembled.py`

### The critical insight: primitive ≠ just CC=1

A function is primitive only if:
- CC=1 (no branching)
- AND no cross-module dependencies (only imports types/stdlib)

CC=1 functions that import from other modules belong at `simple` level. This was the most common mistake in regin's migration — putting CC=1 functions with deps at primitive, then hitting import boundary violations.

### The same-level import ban

**This is the primary constraint that prevents monolith formation.**

`composed.py` in module A CANNOT import from `composed.py` in module B. If it needs to, the options are:

1. **Move the needed function down to simple** (if its CC allows)
2. **Dependency injection** — pass the function as a parameter from the orchestrate layer
3. **Move the function to the same module** (if it belongs there)

DI pattern example from regin:
```python
# orchestrate/level3_permissions_resolve.py (can see everything)
from logic.pure.path_operations.composed import intersect_paths_capabilities, collapse_path_containment
from logic.pure.grant_derive.assembled import derive_explicit_grants

# Injects path_operations functions into grant_derive
grants = derive_explicit_grants(
    security,
    intersect_fn=intersect_paths_capabilities,  # DI
    collapse_fn=collapse_path_containment,       # DI
)
```

### Type aliases for DI contracts

When a function signature gets unwieldy from DI Callable types, define type aliases at the simple level:

```python
# path_operations/simple.py
type PathIntersector = Callable[
    [list[str], list[str], set[Capabilities], set[Capabilities]],
    dict[str, set[str]],
]
```

Then assembled.py imports the type alias instead of spelling out the full Callable signature.

### Pydantic models for parameter grouping

When a function accumulates 5+ parameters from DI + data, group related params into a frozen Pydantic model in `structure/model/`:

```python
class AnthropicContext(BaseModel):
    model_config = ConfigDict(frozen=True)
    model: AnthropicModel
    resolved_tool: ResolvedTool | None = None
    merged_tools: ToolPathEntries | None = None
    display: DisplayEntries | None = None
    hooks: HooksConfig
```

**Exception:** parameters that include `ModuleType` or `type[]` can't be Pydantic models (structure zone can't import runtime types). Accept the param_count warning with documented deferral.

---

## Phase 3: Fix Violations (iterative)

After zones and levels are set, run `saga . --force && syn .` and fix what gleipnir reports.

### Expected violation categories (in order of frequency)

1. **v2_import_boundaries** — cross-level or cross-zone imports that violate the hierarchy. Fix by moving functions to the right level or using DI.
2. **v2_cc_level** — function CC exceeds the level's allowed band. Fix by decomposing the function or moving it to a higher level.
3. **short_param_names / short_local_names** — abbreviated names from the refactor. Rename.
4. **param_count** — accumulated parameters. Group into Pydantic models or use DI.
5. **v2_classes_only_in_structure** — class definitions outside structure/. Move them.

### The fix loop

```
saga . --force --kind py
syn .
# read ALL violations, find the pattern
# fix the architectural root cause (not individual violations)
# test E2E
# repeat
```

Do NOT fix violations one by one. Cluster by type and location. An import boundary violation often means a function is at the wrong level — fixing the level fixes 3-5 violations at once.

---

## Phase 4: Documentation

After the code is structurally stable, upgrade documentation. This is the cheap part — documentation doesn't count against LOC thresholds.

### What to document

- **Module docstrings**: CC level, import rules, what domain problem this solves, how it fits in the pipeline
- **Function docstrings**: what the return value means, what None signals, input contracts, WHY this computation exists
- **Inline comments**: domain rules encoded in data structures, non-obvious algorithm choices

### The documentation test

Read each module docstring in isolation. Could a new session understand what this module does and where it fits without reading the code? If not, the docstring needs work.

---

## Checklist

```
[ ] v1 baseline: saga + syn violation count recorded
[ ] Project added to v2_projects.toml
[ ] structure/ directory created (gen/schema/, gen/fragment/, model/, config/)
[ ] Generated models moved to structure/gen/
[ ] Hand-written models moved to structure/model/
[ ] logic/ directory created (pure/, impure/, transform/, orchestrate/)
[ ] Each v1 file assigned to a zone
[ ] Related files merged into action-named module directories
[ ] Imports updated, E2E passes with new zone layout
[ ] Old directories deleted
[ ] CC measured for every function
[ ] Files split into primitive/simple/composed/assembled levels
[ ] Same-level cross-module imports resolved (DI or level adjustment)
[ ] saga --force + syn shows only expected warnings
[ ] Documentation pass: module docstrings, function docstrings, inline comments
[ ] Final E2E test
[ ] Commit and push
```

---

## Timing expectations

For a project the size of regin (~40 logic files, ~2400 LOC):
- Phase 1 (zones): 1-2 sessions
- Phase 2 (CC levels): 2-3 sessions
- Phase 3 (violations): 1-2 sessions (overlaps with phase 2)
- Phase 4 (documentation): 1 session

Total: 4-6 focused sessions across multiple days. Do not attempt in one sitting — context exhaustion causes architectural mistakes that compound.
