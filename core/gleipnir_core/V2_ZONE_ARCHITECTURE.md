# V2 Zone Architecture — FP Enforcement via Path Classification

## Why This Exists

LLMs generate code sequentially with no persistent working memory. Training data is dominated by OOP examples. When guardrails constrain obvious OOP patterns, the LLM distributes monolith relationships across many small functions that each individually look pure. The functions are clean. The call graph reconstructs the monolith topology. No single node or edge violates anything — but the architecture is fractured OOP in plain sight.

Rules and primers degrade as context fills. Heuristic guardrails catch structural violations but not paradigm-correct syntax with wrong semantics. The solution is an architecture where the directory structure itself makes violations immediately detectable through two simple, orthogonal classification checks.

## The Root Cause

The deepest source of drift was an architectural gap. The LLM had nowhere legal to put orchestration logic or transform logic, so it distributed both across `logic/pure/` — the only overflow valve available. The fragmented monolith was the LLM solving real problems in the worst possible place.

This architecture formalizes two new zones (orchestrate, transform) and enforces decomposition through cyclomatic complexity bounds — not by instruction but by making the wrong thing more expensive than the right thing.

---

## Directory Structure

```
project/
├── structure/
│   └── *.py                    # Pydantic models and enums ONLY
│
└── logic/
    ├── orchestrate/
    │   └── {module}/
    │       ├── orchestrate.py  # CC=1-5, pipeline wiring
    │       └── dispatch.py     # CC=1-2, routing tables for orchestration
    │
    ├── transform/
    │   └── {module}/
    │       ├── primitive.py    # CC=1, no cross-module deps
    │       ├── simple.py       # CC=1-3
    │       ├── dispatch.py     # CC=1-2, thin routing
    │       ├── composed.py     # CC=4-8
    │       └── assembled.py    # CC=1-2, thin composition
    │
    ├── impure/
    │   └── {module}/
    │       ├── primitive.py
    │       ├── simple.py
    │       ├── dispatch.py
    │       ├── composed.py
    │       └── assembled.py
    │
    └── pure/
        └── {module}/
            ├── primitive.py
            ├── simple.py
            ├── dispatch.py
            ├── composed.py
            └── assembled.py
```

Not every module needs all 5 levels. Most modules have 2-3 level files. The module directory name describes the action (e.g., `grant_expand/`, `section_regroup/`), not the pipeline stage it serves.

---

## Zone Definitions

**`structure/`** — Data shape definitions only. Pydantic models (frozen) and enums. No functions, no logic, no calls. No module-level constants (frozen sets, dicts, lookup tables) — all data must be expressed through the type system as enum member values or model field defaults. If gleipnir sees a module-level binding that isn't a class definition, it's a violation.

**`logic/pure/`** — Pure business logic. Deterministic, no side effects, no IO. Purity is enforced by the import graph: a function that imports from impure/ at any level is by definition impure and doesn't belong here.

**`logic/impure/`** — Logic that touches IO, external calls, system state, or any effect. Impurity is declared at the primitive/ffi level and propagates upward through the DAG naturally.

**`logic/transform/`** — Shape conversion. Takes typed structure in, produces typed structure out. No business logic, no IO. Fully isolated from pure/ and impure/ — cannot import from either, and neither can import from transform. Only orchestrate/ can reach transform/. This isolation prevents the LLM from using transforms as building blocks for monolith reconstruction in other zones.

Transforms operate on already-validated typed data, NOT raw input. The pattern is: schema validate first (permissive entry, union types), then transform (normalize to strict internal type). Pydantic BeforeValidators are replaced by explicit transform calls in orchestrate/.

Transform functions have **stricter CC and nesting limits** than pure/impure at the same level. A transform should be a clean shape-to-shape mapping — if it needs deep nesting or complex branching, the logic belongs in pure/ and the transform should call it via orchestrate. The tighter bounds prevent transforms from accumulating business logic disguised as shape conversion.

**`logic/orchestrate/`** — Pipeline coordination. Knows the sequence — which zones are called in what order, how data moves between them. Contains no business logic, no shape definitions, no IO, no transformation. Every line is a function call, a variable binding, a conditional branch, or a return.

Orchestrate uses the same `{module}/` directory + filename-as-level convention as other zones. Two levels are supported:
- **`orchestrate.py`** (CC=1-5) — Pipeline wiring. Calls functions from across the entire stack.
- **`dispatch.py`** (CC=1-2) — Routing tables for orchestration. Same rules as dispatch in other zones (typed dispatch tables only, no functions/classes), but can dispatch more complex callables since it sits at the orchestrate zone level.

Orchestrate can import from any lower level in any reachable zone. This relaxed import rule exists because orchestrate's job is to wire together functions from across the entire stack — restricting it to composed-only forced IO boundary functions (gates, registry) to be artificially inflated to composed level.

This zone's absence caused the fragmented monolith problem. Without it, orchestration logic had nowhere legal to live and distributed itself invisibly across pure/.

**`logic/*/ffi/`** — PyO3 Rust functions and other FFI bindings. The internals are a black box from Python's perspective — purity cannot be inferred and must be asserted by placement (pure/ffi/ vs impure/ffi/ vs transform/ffi/). FFI bindings are peers of primitives: CC=1, neither imports the other, both feed into simple/.

---

## Import Enforcement — Two Orthogonal Axes

Imports are governed by two independent rules. Both must pass for an import to be legal. Their intersection generates the complete import map — no special cases, no exceptions.

### Axis 1: Level Matrix (universal, applies in every zone)

Levels from bottom to top:

| Level | Can import from |
|-------|----------------|
| structure/ | structure/ (free internal imports) |
| ffi/ | structure/ |
| primitive/ | structure/ |
| simple/ | primitive/, ffi/, structure/ |
| dispatch/ | simple/, primitive/, ffi/, structure/ |
| composed/ | dispatch/, simple/, primitive/, ffi/, structure/ |
| assembled/ | composed/, dispatch/, simple/, primitive/, ffi/, structure/ |
| orchestrate/ | assembled/, composed/, dispatch/, simple/, primitive/, ffi/, structure/ |
| entry_point (cli.py, __main__.py) | orchestrate/, structure/ |

**Rules:**
- No same-level imports — ever. This is the primary anti-monolith binding. Same-level imports are how the LLM rebuilds OOP clusters without triggering alarms.
- ffi/ and primitive/ are peers — neither imports the other. If a function needs ffi + native logic, it belongs in simple/.
- Dispatch and assembled are symmetric thin layers. Dispatch routes between simples (CC=1-2). Assembled composes from composed (CC=1-2). Both exist to give thin wiring functions a proper home at the right level.
- Orchestrate can reach any lower level. Its job is to wire the entire stack — restricting it to one level down would force functions to be artificially inflated to satisfy consumption requirements.
- Entry point (cli.py, __main__.py at project root) can only reach orchestrate — it is the thinnest possible wrapper (CC=1-2). No `entry_point.py` inside zones — that creates shim files.

### Axis 2: Zone Matrix (which tracks can see which)

| Zone | Can import from zones |
|------|----------------------|
| pure/ | pure/ |
| impure/ | impure/, pure/ |
| transform/ | transform/ |
| orchestrate/ | pure/, impure/, transform/ |

**Rules:**
- Pure is walled — cannot see impure or transform.
- Impure can reach into pure (at strictly lower levels, which falls out automatically from the same-level ban).
- Transform is fully isolated — no edges to/from pure or impure.
- Orchestrate sees all three zone tracks.
- Structure/ is accessible to everything via the level rules.

### How the Intersection Works

A legal import must pass BOTH tables. Examples:

- `impure/simple/` importing `pure/primitive/` → Level: simple > primitive ✓, Zone: impure→pure ✓ → **Legal**
- `impure/simple/` importing `pure/simple/` → Level: same-level ✗ → **Illegal** (the cross-track "strictly lower" rule falls out automatically)
- `transform/simple/` importing `pure/primitive/` → Level: simple > primitive ✓, Zone: transform→pure ✗ → **Illegal** (transform isolation)
- `orchestrate/` importing `pure/composed/` → Level: orchestrate > composed ✓, Zone: orchestrate→pure ✓ → **Legal**
- `orchestrate/` importing `impure/simple/` → Level: orchestrate > simple ✓, Zone: orchestrate→impure ✓ → **Legal** (relaxed orchestrate)
- `transform/assembled/` importing `transform/composed/` → Level: assembled > composed ✓, Zone: transform→transform ✓ → **Legal**
- `pure/composed/` importing `impure/primitive/` → Level: composed > primitive ✓, Zone: pure→impure ✗ → **Illegal**

---

## Cyclomatic Complexity Enforcement

CC measures linearly independent paths. CC=1 is a straight path. Every branch adds 1. This is fundamentally different from LOC — LOC says "be short," the LLM responds by being dense. CC=1 says "have no branching" — much harder to game.

| Location | CC | Role |
|----------|-----|------|
| structure/ | N/A | Data shape definitions only |
| ffi/ | 1 | Black-box FFI bindings |
| primitive/ | 1 | Single-expression, no cross-module deps |
| simple/ | 1-3 | Building blocks with deps |
| dispatch/ | 1-2 | Thin routing between simples |
| composed/ | 4-8 | Multi-branch business logic |
| assembled/ | 1-2 | Thin composition of composed functions |
| orchestrate/ | 1-5 | Pipeline wiring across zones |
| entry_point (cli.py, __main__.py) | 1-2 | Thinnest wrapper at project root, calls orchestrate |

### The Gravity Rule

Code must live at the **lowest level it legally can**, not the highest level it is permitted to be at.

- CC=1 in simple/ → gravity violation, must move to primitive/
- CC=2-3 in composed/ → gravity violation, must move to simple/
- CC=4+ in primitive/ → ceiling violation

The gravity rule prevents the LLM from floating everything to composed/ where constraints are loosest. The question at every function is not "can this go here?" but "must this go lower?"

---

## How This Defeats the Fragmented Monolith

**Same-level import ban** — The LLM cannot build object-like clusters because functions at the same level cannot import each other. The monolith topology cannot form.

**Orchestrate/ as formal zone** — Orchestration logic that was previously distributed invisibly across pure/ now has a legal home. Its CC ceiling of 5 allows necessary wiring while preventing logic accumulation.

**Transform isolation** — Shape conversion code is severed from both pure/ and impure/. The LLM cannot use transforms as building blocks for monolith reconstruction because they're behind a one-way wall only orchestrate/ can reach.

**Gravity rule** — Functions must live as low as they can. The LLM cannot park composed logic in primitive/ or simple/ to avoid stricter constraints.

**Two-axis intersection** — Instead of memorizing a 20-entry import map, the LLM internalizes two small tables. The rules are simple enough to follow, which means compliance improves without enforcement increasing.

---

## Gleipnir Enforcement Checklist

On every file save, gleipnir v2 checks:

1. **Level import violation** — import target is at the same or higher level
2. **Zone import violation** — import target is in a zone not reachable from the source zone
3. **Ceiling violation** — function CC exceeds the level's maximum
4. **Gravity violation** — function CC is below the level's minimum (function must be pushed down)
5. **Structure logic violation** — function definition or module-level non-class binding in structure/
6. **Constant in logic violation** — module-level data binding (not a function def) in any logic/ module

LOC limits remain as secondary enforcement — they carry less of the load but still catch the obvious monolith growth.

---

## Relationship to Existing Gleipnir Architecture

This is an extension of the existing classify-then-check pattern. Current gleipnir:

1. `classify_file()` → FileKind (script, project Python, project Rust, etc.)
2. Matrix selects checks by FileKind
3. Run selected checks

V2 adds two classification axes:

1. `classify_file()` → FileKind + Level + Zone
2. Matrix selects checks by FileKind
3. For each import, classify target → Level + Zone
4. Level check: target level < source level? (table lookup)
5. Zone check: target zone reachable from source zone? (table lookup)

Classification from path is deterministic — `logic/pure/tool_resolve/simple.py` → zone=pure, level=simple. The filename encodes the level (primitive, simple, dispatch, composed, assembled). The parent directories encode the zone. No heuristics needed. The checks themselves are trivial comparisons after classification.

---

## Key Design Decisions

**Transform imports structure/ only** — Validated by examining actual codebases (draupnir converters/, regin render_regroup_*.py). Every transform in both projects imports only from structure/. The isolation is practical, not theoretical.

**No Pydantic BeforeValidators** — Transforms are called explicitly: `model.model_validate_json(data)` then `transform(typed_data)`. This makes every boundary crossing a visible node in the call graph. Schema validates first (permissive entry with union types), transform normalizes second (typed dispatch to strict internal type).

**Dispatch and Assembled as symmetric thin layers** — The regin migration revealed a recurring pattern: thin wiring functions (CC=1-2) that connect functions at the level below. Dispatch routes between simples (e.g., selecting which simple function to call based on a key). Assembled composes from composed (e.g., `check_blocking()` that calls three composed check functions and concatenates results). Without these levels, thin compositors were forced into composed with permanent gravity violations — their CC was too low for composed but their import dependencies prevented moving down.

**Relaxed orchestrate imports** — Orchestrate originally could only import composed (strict one-step-down). This forced IO boundary functions (gates, registry) to stay at composed level despite being CC=2-3, because orchestrate needed them and couldn't reach simple. The cascade: gates forced to composed → everything importing gates forced to composed → widespread artificial gravity violations. Relaxing orchestrate to reach any lower level lets functions live at their natural CC level regardless of who consumes them.

**No constants in structure/** — All data expressed through the type system. Frozen sets and lookup tables become enum member values. Module-level bindings that aren't class definitions are violations everywhere.

**V1/V2 coexistence via migration list** — Saga checks project path against a migration list to determine which check version applies. Projects are either fully v1 or fully v2. No mixed signals — the LLM in a v2 project sees only v2 violations.
