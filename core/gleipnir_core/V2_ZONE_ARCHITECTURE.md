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
├── structure/                            # L0 — data shape definitions
│   ├── model/                            # Pydantic models
│   ├── config/                           # Enums and static definitions
│   └── exception/                        # Exception classes
│
└── logic/
    ├── orchestrate/
    │   └── {module}/
    │       ├── orchestrate.py            # L7, CC=1-5, pipeline wiring
    │       └── dispatch.py               # L6, CC=1-2, routing tables
    │
    ├── transform/
    │   └── {module}/
    │       ├── primitive.py              # L1, CC=1
    │       ├── ffi.py                    # L1, CC=1, FFI bindings
    │       ├── simple.py                 # L2, CC=1-3
    │       ├── dispatch.py               # L3, CC=1-2, thin routing
    │       ├── composed.py               # L4, CC=4-8
    │       └── assembled.py              # L5, CC=1-2, thin composition
    │
    ├── impure/
    │   └── {module}/
    │       ├── primitive.py              # L1
    │       ├── ffi.py                    # L1
    │       ├── simple.py                 # L2
    │       ├── dispatch.py               # L3
    │       ├── composed.py               # L4
    │       └── assembled.py              # L5
    │
    └── pure/
        └── {module}/
            ├── primitive.py              # L1
            ├── ffi.py                    # L1
            ├── simple.py                 # L2
            ├── dispatch.py               # L3
            ├── composed.py               # L4
            └── assembled.py              # L5
```

Not every module needs all levels. Most modules have 2-3 level files. The module directory name describes the action (e.g., `grant_expand/`, `section_regroup/`), not the pipeline stage it serves.

---

## Levels — Numbered, Immutable Positions

Levels are numbered positions in the import hierarchy. The number is the level — it never changes. Filenames map to levels, but a filename is not a level. The same filename can map to different levels depending on zone (dispatch.py is L2 in logic zones, L5 in orchestrate zone). Different filenames can map to the same level (primitive.py and ffi.py are both L0).

| Level | Filenames | CC | Role |
|-------|-----------|-----|------|
| L0 | *.py in structure/ | N/A | Data shape definitions only |
| L1 | primitive.py, ffi.py | 1 | Leaf functions, no cross-module deps |
| L2 | simple.py | 1-3 | Building blocks with deps |
| L3 | dispatch.py (logic zones) | 1-2 | Thin routing between L2 functions |
| L4 | composed.py | 4-8 | Multi-branch business logic |
| L5 | assembled.py | 1-2 | Thin composition of L4 functions |
| L6 | dispatch.py (orchestrate zone) | 1-2 | Routing tables for L7 |
| L7 | orchestrate.py | 1-5 | Pipeline wiring across zones |
| L8 | cli.py, __main__.py | 1-2 | Entry point, thinnest wrapper |

### The Gravity Rule

Code must live at the **lowest level it legally can**, not the highest level it is permitted to be at.

- CC=1 in L2 → gravity violation, must move to L1
- CC=2-3 in L4 → gravity violation, must move to L2
- CC=4+ in L1 → ceiling violation

The gravity rule prevents the LLM from floating everything to L4 where constraints are loosest. The question at every function is not "can this go here?" but "must this go lower?"

---

## Import Enforcement — Two Rules

A legal import must satisfy both rules:

### Rule 1: Level — higher imports lower

`source.level > target.level`

A file can only import from files at a strictly lower level number. Same-level imports are forbidden — this is the primary anti-monolith binding.

That's it. No lookup table. No special cases. L6 can import from L5, L4, L3, L2, L1, and L0. L3 can import from L2, L1, and L0. The comparison is the rule.

### Rule 2: Zone visibility

Each zone has a fixed set of zones it can import from:

| Zone | Can import from |
|------|----------------|
| pure | pure |
| transform | transform |
| impure | impure, pure |
| orchestrate | anything |

Structure (L0) is accessible from all zones — it's below every logic level.

### That's the whole system

- **Pure** is walled — cannot reach impure, transform, or orchestrate.
- **Transform** is fully isolated — cannot reach any other zone, and only orchestrate can reach into it.
- **Impure** can reach into pure (at strictly lower levels, which falls out from the level rule) but cannot reach transform.
- **Orchestrate** sees everything.

### How the Three Rules Work Together

A legal import must pass both Rule 1 and Rule 2:

- `impure/L2` importing `pure/L1` → Level: 2 > 1 ✓, Zone: impure is free ✓ → **Legal**
- `impure/L2` importing `pure/L2` → Level: 2 = 2 ✗ → **Illegal** (same-level)
- `transform/L2` importing `pure/L1` → Level: 2 > 1 ✓, Zone: transform is walled ✗ → **Illegal**
- `orchestrate/L7` importing `pure/L4` → Level: 7 > 4 ✓, Zone: orchestrate is free ✓ → **Legal**
- `orchestrate/L6` importing `transform/L5` → Level: 6 > 5 ✓, Zone: orchestrate is free ✓ → **Legal**
- `orchestrate/L7` importing `orchestrate/L6` → Level: 7 > 6 ✓, Zone: orchestrate is free ✓ → **Legal**
- `pure/L4` importing `impure/L1` → Level: 4 > 1 ✓, Zone: pure→impure ✗ → **Illegal**
- `impure/L4` importing `transform/L2` → Level: 4 > 2 ✓, Zone: impure→transform ✗ → **Illegal**
- `transform/L5` importing `transform/L4` → Level: 5 > 4 ✓, Zone: transform→transform ✓ → **Legal**

---

## Zone Definitions

**`structure/`** — Data shape definitions only. Three categories:
- **model/** — Pydantic models (BaseModel, RootModel). Frozen, typed data shapes.
- **config/** — Enums and static type definitions. Configuration expressed through the type system.
- **exception/** — Exception classes. Error types that carry typed diagnostic information.

No functions, no logic, no calls. No module-level constants (frozen sets, dicts, lookup tables) — all data must be expressed through the type system as enum member values or model field defaults. If gleipnir sees a module-level binding that isn't a class definition, it's a violation.

**`logic/pure/`** — Pure business logic. Deterministic, no side effects, no IO. Purity is enforced by the import graph: a function that imports from impure/ at any level is by definition impure and doesn't belong here.

**`logic/impure/`** — Logic that touches IO, external calls, system state, or any effect. Impurity is declared at the L1 level and propagates upward through the DAG naturally.

**`logic/transform/`** — Shape conversion. Takes typed structure in, produces typed structure out. No business logic, no IO. Fully isolated — cannot import from pure/ or impure/, and neither can import from transform/. Only orchestrate/ can reach transform/. This isolation prevents the LLM from using transforms as building blocks for monolith reconstruction in other zones.

Transforms operate on already-validated typed data, NOT raw input. The pattern is: schema validate first (permissive entry, union types), then transform (normalize to strict internal type). Pydantic BeforeValidators are replaced by explicit transform calls in orchestrate/.

Transform functions have **stricter CC and nesting limits** than pure/impure at the same level. A transform should be a clean shape-to-shape mapping — if it needs deep nesting or complex branching, the logic belongs in pure/ and the transform should call it via orchestrate. The tighter bounds prevent transforms from accumulating business logic disguised as shape conversion.

**`logic/orchestrate/`** — Pipeline coordination. Knows the sequence — which zones are called in what order, how data moves between them. Contains no business logic, no shape definitions, no IO, no transformation. Every line is a function call, a variable binding, a conditional branch, or a return.

Two levels:
- **L7 — orchestrate.py** (CC=1-5) — Pipeline wiring. Calls functions from across the entire stack.
- **L6 — dispatch.py** (CC=1-2) — Routing tables for orchestration. Typed dispatch dicts mapping keys to callables. No functions, no classes — just imports and typed table assignments. Can reference functions from any level below (L5 and down) because it sits above the logic zone ceiling.

**`logic/*/ffi.py`** — PyO3 Rust functions and other FFI bindings. The internals are a black box from Python's perspective — purity cannot be inferred and must be asserted by placement (pure/ffi.py vs impure/ffi.py vs transform/ffi.py). FFI files are at L1, the same level as primitive — CC=1, both feed into L2 (simple).

---

## Classification

### File → Level

Classification is deterministic from (filename, zone):

| Filename | Zone | Level |
|----------|------|-------|
| *.py | structure/ | L0 |
| primitive.py | pure, impure, transform | L1 |
| ffi.py | pure, impure, transform | L1 |
| simple.py | pure, impure, transform | L2 |
| dispatch.py | pure, impure, transform | L3 |
| composed.py | pure, impure, transform | L4 |
| assembled.py | pure, impure, transform | L5 |
| dispatch.py | orchestrate | L6 |
| orchestrate.py | orchestrate | L7 |
| cli.py | (project root) | L8 |
| __main__.py | (project root) | L8 |
| __init__.py | (any zone) | Outside (no checks) |

### Zone from Path

| Path contains | Zone |
|---------------|------|
| /logic/pure/ | Pure |
| /logic/impure/ | Impure |
| /logic/transform/ | Transform |
| /logic/orchestrate/ | Orchestrate |
| /structure/ | Structure |

---

## How This Defeats the Fragmented Monolith

**Level ordering** — The LLM cannot build object-like clusters because same-level imports are impossible. `source > target` is a total order — the monolith topology cannot form because it requires lateral edges. Structure at L0 is naturally accessible from every logic level (L1+) without special cases.

**Orchestrate/ as formal zone** — Orchestration logic that was previously distributed invisibly across pure/ now has a legal home. Its CC ceiling of 5 allows necessary wiring while preventing logic accumulation.

**Transform isolation** — Shape conversion code is severed from both pure/ and impure/. The LLM cannot use transforms as building blocks for monolith reconstruction because they're behind a wall only orchestrate/ can reach.

**Gravity rule** — Functions must live as low as they can. The LLM cannot park composed logic in L1 or L2 to avoid stricter constraints.

**Two-rule simplicity** — Instead of memorizing a lookup table, the LLM internalizes one numeric comparison and a small zone visibility table. The rules are simple enough to follow without degradation over long contexts.

---

## Gleipnir Enforcement Checklist

On every file save, gleipnir v2 checks:

1. **Level import violation** — target level >= source level (must be strictly lower)
2. **Zone import violation** — pure importing outside pure, or transform importing outside transform
3. **Ceiling violation** — function CC exceeds the level's maximum
4. **Gravity violation** — function CC is below the level's minimum (function must be pushed down)
5. **Structure logic violation** — function definition or module-level non-class binding in structure/
6. **Constant in logic violation** — module-level data binding (not a function def) in any logic/ module (except dispatch files which hold typed tables)

---

## Key Design Decisions

**Numbered levels, not named levels** — Level names (Dispatch, Assembled, Orchestrate) created false equivalences between filenames and positions. The same filename (dispatch.py) maps to different levels depending on zone. Different filenames (primitive.py, ffi.py) map to the same level. Numbered levels make the import rule a single comparison and eliminate the mental mapping entirely.

**Transform imports structure/ only** — Validated by examining actual codebases (draupnir converters/, regin render_regroup_*.py). Every transform in both projects imports only from structure/. The isolation is practical, not theoretical.

**No Pydantic BeforeValidators** — Transforms are called explicitly: `model.model_validate_json(data)` then `transform(typed_data)`. This makes every boundary crossing a visible node in the call graph. Schema validates first (permissive entry with union types), transform normalizes second (typed dispatch to strict internal type).

**Dispatch and Assembled as symmetric thin layers** — Both are CC=1-2 thin wiring at different positions in the hierarchy. Dispatch (L3) routes between simples (L2). Assembled (L5) composes from composed (L4). Orchestrate-zone dispatch (L6) routes for orchestrate (L7). The pattern repeats at three points in the hierarchy.

**Relaxed orchestrate imports** — Orchestrate originally could only import composed (strict one-step-down). This forced IO boundary functions (gates, registry) to stay at composed level despite being CC=2-3, because orchestrate needed them and couldn't reach simple. The cascade: gates forced to composed → everything importing gates forced to composed → widespread artificial gravity violations. Level numbering eliminates this — L7 can reach any level below it naturally.

**No constants in structure/** — All data expressed through the type system. Frozen sets and lookup tables become enum member values. Module-level bindings that aren't class definitions are violations everywhere.

**Three rules replace a lookup table** — The old system required memorizing which named levels could import which other named levels. The new system is: higher number imports lower number, pure is walled, transform is walled. Any LLM can hold three rules in context indefinitely, even under context pressure.
