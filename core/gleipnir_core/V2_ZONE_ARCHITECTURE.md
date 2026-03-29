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
    ├── orchestrate/            # CC=1-2, flat (no sub-levels)
    │
    ├── transform/
    │   ├── ffi/                # CC=1, black box
    │   ├── primitive/          # CC=1
    │   ├── simple/             # CC=2-3
    │   └── composed/           # CC=4+
    │
    ├── impure/
    │   ├── ffi/                # CC=1, black box
    │   ├── primitive/          # CC=1
    │   ├── simple/             # CC=2-3
    │   └── composed/           # CC=4+
    │
    └── pure/
        ├── ffi/                # CC=1, black box
        ├── primitive/          # CC=1
        ├── simple/             # CC=2-3
        └── composed/           # CC=4+
```

---

## Zone Definitions

**`structure/`** — Data shape definitions only. Pydantic models (frozen) and enums. No functions, no logic, no calls. No module-level constants (frozen sets, dicts, lookup tables) — all data must be expressed through the type system as enum member values or model field defaults. If gleipnir sees a module-level binding that isn't a class definition, it's a violation.

**`logic/pure/`** — Pure business logic. Deterministic, no side effects, no IO. Purity is enforced by the import graph: a function that imports from impure/ at any level is by definition impure and doesn't belong here.

**`logic/impure/`** — Logic that touches IO, external calls, system state, or any effect. Impurity is declared at the primitive/ffi level and propagates upward through the DAG naturally.

**`logic/transform/`** — Shape conversion. Takes typed structure in, produces typed structure out. No business logic, no IO. Fully isolated from pure/ and impure/ — cannot import from either, and neither can import from transform. Only orchestrate/ can reach transform/. This isolation prevents the LLM from using transforms as building blocks for monolith reconstruction in other zones.

Transforms operate on already-validated typed data, NOT raw input. The pattern is: schema validate first (permissive entry, union types), then transform (normalize to strict internal type). Pydantic BeforeValidators are replaced by explicit transform calls in orchestrate/.

**`logic/orchestrate/`** — Pipeline coordination. Knows the sequence — which zones are called in what order, how data moves between them. Contains no business logic, no shape definitions, no IO, no transformation. Every line is a function call, a variable binding, a conditional branch, or a return. CC=1-2 ceiling makes it physically impossible to accumulate logic here.

This zone's absence caused the fragmented monolith problem. Without it, orchestration logic had nowhere legal to live and distributed itself invisibly across pure/.

**`logic/*/ffi/`** — PyO3 Rust functions and other FFI bindings. The internals are a black box from Python's perspective — purity cannot be inferred and must be asserted by placement (pure/ffi/ vs impure/ffi/ vs transform/ffi/). FFI bindings are peers of primitives: CC=1, neither imports the other, both feed into simple/.

---

## Import Enforcement — Two Orthogonal Axes

Imports are governed by two independent rules. Both must pass for an import to be legal. Their intersection generates the complete import map — no special cases, no exceptions.

### Axis 1: Level Matrix (universal, applies in every zone)

Levels from bottom to top:

| Level | Can import from |
|-------|----------------|
| structure/ | nothing |
| ffi/ | structure/ |
| primitive/ | structure/ |
| simple/ | primitive/, ffi/, structure/ |
| composed/ | simple/, primitive/, ffi/, structure/ |
| orchestrate/ | composed/, structure/ |
| entry point | orchestrate/, structure/ |

**Rules:**
- No same-level imports — ever. This is the primary anti-monolith binding. Same-level imports are how the LLM rebuilds OOP clusters without triggering alarms.
- ffi/ and primitive/ are peers — neither imports the other. If a function needs ffi + native logic, it belongs in simple/.
- Orchestrate/ reaches only composed/ — not simple/ or primitive/ directly. This prevents sequential smuggling (stringing together many primitive calls at CC=1).

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
- `pure/composed/` importing `impure/primitive/` → Level: composed > primitive ✓, Zone: pure→impure ✗ → **Illegal**

---

## Cyclomatic Complexity Enforcement

CC measures linearly independent paths. CC=1 is a straight path. Every branch adds 1. This is fundamentally different from LOC — LOC says "be short," the LLM responds by being dense. CC=1 says "have no branching" — much harder to game.

| Location | CC |
|----------|-----|
| structure/ | N/A (no functions) |
| ffi/ | exactly 1 |
| primitive/ | exactly 1 |
| simple/ | 2-3 |
| composed/ | 4+ |
| orchestrate/ | 1-2 |

### The Gravity Rule

Code must live at the **lowest level it legally can**, not the highest level it is permitted to be at.

- CC=1 in simple/ → gravity violation, must move to primitive/
- CC=2-3 in composed/ → gravity violation, must move to simple/
- CC=4+ in primitive/ → ceiling violation

The gravity rule prevents the LLM from floating everything to composed/ where constraints are loosest. The question at every function is not "can this go here?" but "must this go lower?"

---

## How This Defeats the Fragmented Monolith

**Same-level import ban** — The LLM cannot build object-like clusters because functions at the same level cannot import each other. The monolith topology cannot form.

**Orchestrate/ as formal zone** — Orchestration logic that was previously distributed invisibly across pure/ now has a legal home. Its CC ceiling of 1-2 makes it physically impossible to accumulate logic there.

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

Classification from path is deterministic — `logic/pure/simple/foo.py` → zone=pure, level=simple. No heuristics needed. The checks themselves are trivial comparisons after classification.

---

## Key Design Decisions

**Transform imports structure/ only** — Validated by examining actual codebases (draupnir converters/, regin render_regroup_*.py). Every transform in both projects imports only from structure/. The isolation is practical, not theoretical.

**No Pydantic BeforeValidators** — Transforms are called explicitly: `model.model_validate_json(data)` then `transform(typed_data)`. This makes every boundary crossing a visible node in the call graph. Schema validates first (permissive entry with union types), transform normalizes second (typed dispatch to strict internal type).

**No constants in structure/** — All data expressed through the type system. Frozen sets and lookup tables become enum member values. Module-level bindings that aren't class definitions are violations everywhere.

**V1/V2 coexistence via migration list** — Saga checks project path against a migration list to determine which check version applies. Projects are either fully v1 or fully v2. No mixed signals — the LLM in a v2 project sees only v2 violations.
