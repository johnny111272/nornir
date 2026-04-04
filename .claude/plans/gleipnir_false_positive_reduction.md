# Gleipnir Rust False Positive Reduction — Execution Plan

Source: `core/gleipnir_core/RUST_FALSE_POSITIVE_REDUCTION.md` by Eitri
Research: verified against actual syn output (29 violations) and source code

---

## Current State (verified via syn)

| Check | Count | Files |
|-------|------:|-------|
| function_length_rs | 6 | 5 files (NOT in scope) |
| no_clone_spam | 9 | 6 files |
| no_println | 4 | 2 files |
| no_string_abuse | 10 | 7 files |
| **Total** | **29** | |

In-scope: 23 violations across no_clone_spam, no_println, no_string_abuse.

---

## Corrections to Eitri's Document

### 1. annotation.rs:176 is a Change 1 pattern, not Change 3

The doc categorizes this under Change 3 as "clone in FieldLineParse::Error." But the actual code at line 175-176 is:
```rust
let field_path = if section_path.is_empty() {
    field_name.clone()   // ← line 176 (if-else let binding)
} else {
    format!("{}.{}", section_path, field_name)
};
```
This is the if-else value-producing position pattern — Change 1 resolves it.

### 2. convert.rs has TWO violations, not one

- Line 191: `"(root)".to_string()` in if-else (no_string_abuse) — Change 1
- Line 199: `k.clone()` in if-else (no_clone_spam) — Change 1

The doc only lists 199. Line 191 is also in the Change 1 pattern (`.push(if ... { "x".to_string() } else { ... })` — the if-else is an argument to `.push()` which is a call_expression, so actually this may already be caught via call_expression. Let me verify: walk from to_string() → block → is_implicit_return → parent is if_expression → false. So it falls through. The call_expression (.push) is above the if_expression, unreachable. Change 1 fix IS needed for this one.

### 3. Tail-expression gap in proposed `is_value_producing_position`

Eitri's proposed function checks whether the outermost if_expression's parent is in a list (`let_declaration`, `return_expression`, etc.). It MISSES the case where the if-else is the tail expression of a function body:

```rust
fn normalize(path: &str) -> String {
    if x { "a".to_string() } else { "b".to_string() }
}
```

Here the outermost if_expression's parent is `block` (the function body), which is not in the consumer list. Need to add: if parent is `block`, delegate to `is_implicit_return` to check if that block is a function/closure/match-arm body with this if_expression as the last expression.

### 4. `let mut` detection must use tree-sitter nodes, not text matching

Eitri proposes `text.contains("let mut")`. This is fragile — could match variable names or comments. Tree-sitter provides a `mutable_specifier` named child on `let_declaration`. Check for that node instead.

---

## Revised Violation Resolution Map

After Change 1 (if-else value-producing position) — **12 violations resolved**:

| File | Line | Check | Pattern |
|------|------|-------|---------|
| format_core/tomlx/diagnostics.rs | 264 | string_abuse | `"none".to_string()` in if-else let |
| format_core/tomlx/diagnostics.rs | 282 | string_abuse | same pattern |
| format_core/tomlx/paths.rs | 180 | string_abuse | `".".to_string()` in else-if |
| format_core/convert.rs | 191 | string_abuse | `"(root)".to_string()` in if-else push() |
| format_core/convert.rs | 199 | clone_spam | `k.clone()` in if-else let |
| error_core/src/lib.rs | 129 | string_abuse | `"/".to_string()` in if-else let |
| error_core/src/lib.rs | 346 | string_abuse | `"/".to_string()` in if-else let |
| datagram_io/src/lib.rs | 267 | string_abuse | `"@".to_string()` in if-else let |
| datagram_io/src/lib.rs | 272 | string_abuse | `"@".to_string()` in if-else let |
| report_render_core/src/lib.rs | 330 | clone_spam | `group.code.clone()` in if-else |
| hook_pre_subagent_bash/src/main.rs | 277 | string_abuse | `"  (none)".to_string()` in if-else let |
| format_core/tomlx/annotation.rs | 176 | clone_spam | `field_name.clone()` in if-else let |

After Change 2 (tier-aware no_println) — **4 violations resolved**:

| File | Lines | Check |
|------|-------|-------|
| cli/syn_cli/src/main.rs | 340, 347, 356 | no_println |
| cli/saga_cli/src/main.rs | 182 | no_println |

After Change 3 (let-mut clone detection) — **2 violations resolved**:

| File | Line | Check | Pattern |
|------|------|-------|---------|
| diff_core/src/lib.rs | 112 | clone_spam | `let mut stripped = value.clone()` |
| format_core/tomlx/types.rs | 237 | clone_spam | `let mut output = self.data.clone()` |

**Remaining after all changes: 5 violations**

| File | Line | Check | Status |
|------|------|-------|--------|
| report_render_core/src/lib.rs | 47 (x2) | clone_spam | Known deferred (BTreeMap key) |
| report_render_core/src/lib.rs | 396 | string_abuse | Known deferred |
| format_core/tomlx/validation.rs | 128 | clone_spam | Needs investigation (assignment into struct field) |
| diff_core/src/lib.rs | 182 | clone_spam | Known deferred (clone-before-overwrite, needs CFG) |

**Expected result: 29 → 11** (6 function_length + 5 remaining)

---

## Execution Steps

All work in one file: `core/gleipnir_core/src/checks_rs/prohibited.rs`

### Step 1: Add `is_value_producing_position` function

New function replacing `is_implicit_return` in the clone/string ownership walk. Handles:
- Existing: function_item, match_arm, closure_expression (delegate to existing last-expression check)
- NEW: if_expression / else_clause parent → walk up to outermost if_expression → check if that if_expression is consumed by a let_declaration, return_expression, field_initializer, arguments, call_expression, match_arm, tuple_expression, array_expression — OR if it's the tail expression in a function/closure/match-arm block

Implementation detail on the tail-expression case:
```
if outermost_if_expr.parent() is "block" {
    // Check if the block is a function/closure/match body
    // AND the if_expression is the last named child of that block
    is_implicit_return(block, outermost_if_expr)
}
```

This reuses the existing `is_implicit_return` logic for the final check, keeping it DRY.

### Step 2: Update `clone_is_ownership_transfer` block arm

Change the `"block"` arm at line 418:
```rust
"block" => return is_implicit_return(node, clone_call),
```
to:
```rust
"block" => return is_value_producing_position(node, clone_call),
```

The new function subsumes `is_implicit_return` — it handles both the direct case (block is function body) and the new case (block is if-else branch in a value-producing position).

### Step 3: Add `let_declaration` with `mutable_specifier` detection

In `clone_is_ownership_transfer`, add before the catch-all `_ =>`:
```rust
"let_declaration" => {
    // let mut x = val.clone() — cloning to mutate is intentional
    let mut cursor = node.walk();
    let has_mut = node.children(&mut cursor)
        .any(|c| c.kind() == "mutable_specifier");
    return has_mut;
}
```

Returns `true` (ownership transfer / skip) only if `let mut`. Falls through to `false` (violation) for plain `let`.

### Step 4: Add `RustCrateTier` and tier-aware println skip

Add to `prohibited.rs` (not classify.rs — that's Python-only):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustCrateTier {
    Core,
    Capability,
    Binary,
    Gate,
    Unknown,
}

fn classify_rust_tier(file_path: &str) -> RustCrateTier { ... }
fn in_run_function(node, source) -> bool { ... }
```

In `check_no_println`, after the existing `in_main_function` / `in_output_function` check:
```rust
if name == "println"
    && classify_rust_tier(source.file_path) == RustCrateTier::Binary
    && in_run_function(node, source.source_bytes)
{
    continue;
}
```

`dbg!` remains always-flagged (no run() exemption).

### Step 5: Add tests

Group 1 — if-else value position (6 tests):
- `to_string_in_if_else_let_binding_ok` — must NOT flag
- `clone_in_if_else_let_binding_ok` — must NOT flag
- `to_string_in_else_if_chain_ok` (tail expression) — must NOT flag
- `to_string_in_if_else_function_arg_ok` (.push(if {...})) — must NOT flag
- `to_string_in_plain_let_binding_still_caught` — must STILL flag
- `to_string_in_if_body_not_value_position_still_caught` — must STILL flag

Group 2 — tier-aware println (5 tests):
- `println_in_run_function_binary_crate_ok` — must NOT flag
- `println_in_run_function_library_crate_caught` — must STILL flag
- `println_in_helper_function_binary_crate_caught` — must STILL flag
- `dbg_in_run_function_binary_crate_still_caught` — must STILL flag
- `tier_classification` — unit test for classify_rust_tier

Group 3 — let-mut clone (2 tests):
- `clone_into_let_mut_ok` — must NOT flag
- `clone_into_let_immutable_still_caught` — must STILL flag

### Step 6: Run tests and verify

```
cargo test -p gleipnir_core
```

All existing tests must pass (no regressions). All new tests must pass.

### Step 7: Rebuild and verify violation count

```
nornir_deploy --build gates
nornir_deploy --build tools
syn .
```

Expected: total drops from 29 to 11. The 6 function_length violations are unchanged. The remaining 5 clone/string violations are genuine or known-deferred.

---

## Risk Assessment

**Low risk**: All changes are additive (new detection paths that return `true` to skip violations). No existing `true` → `false` path changes, so no new violations can appear on previously-clean code.

**Regression guard**: The "must STILL flag" tests verify that real violations remain caught. The existing test suite (30+ tests) covers all current detection paths.

**Scope containment**: All changes in one file (prohibited.rs). No structural changes, no new crates, no new dependencies.
