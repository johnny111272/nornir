# Rust False Positive Reduction Plan

Authored by Eitri for Tyr to implement.

## Problem

syn reports 29 violations across the nornir workspace. Approximately 15 are false positives caused by detection gaps in three checks. The checks correctly identify the syntactic pattern but lack context to determine whether the pattern is intentional.

The goal is NOT to suppress violations. It is to make the checks smarter so they find real issues and skip legitimate code. Every change must be backed by a test case showing the false positive AND a test case showing a real violation that must still be caught.

---

## Change 1: if-else branch ownership (clone + string_abuse)

### The gap

`clone_is_ownership_transfer()` in `checks_rs/prohibited.rs` (line 393) walks up the AST to determine if a `.clone()` or `.to_string()` flows into an ownership-requiring context. It recognizes struct fields, function arguments, return expressions, match arms, collection methods, and implicit returns.

It does NOT recognize **if-else expressions used as value producers**. When `.to_string()` or `.clone()` appears inside one branch of an if-else, the expression as a whole produces an owned value. The allocation is required because the other branch produces a `String` and both branches must have the same type.

### The AST walk failure

```rust
let defined = if issue.defined_refs.is_empty() {
    "none".to_string()   // <-- flagged
} else {
    issue.defined_refs.join(", ")
};
```

Walk from `.to_string()`:
1. Parent: `block` (the `{ "none".to_string() }` if-branch body)
2. `block` triggers `is_implicit_return()` which checks if parent is `function_item | match_arm | closure_expression`
3. Parent is `if_expression` -- not recognized. Returns `false`.
4. Falls through to `_ => return false`.

The same pattern appears in `clone`:
```rust
let short = if !group.representative_message.is_empty() {
    // ... truncation logic ...
    short.to_string()   // <-- flagged
} else {
    group.code.clone()  // <-- flagged
};
```

### Affected violations (10 of 29)

| File | Line | Pattern |
|------|------|---------|
| format_core/tomlx/diagnostics.rs | 264 | `"none".to_string()` in if-else let binding |
| format_core/tomlx/diagnostics.rs | 282 | same pattern (different branch) |
| format_core/tomlx/paths.rs | 180 | `".".to_string()` in else-if branch |
| format_core/convert.rs | 199 | `k.clone()` in iterator `.map()` closure -- see note below |
| error_core/src/lib.rs | 129 | `"/".to_string()` in if-else let binding |
| error_core/src/lib.rs | 346 | `"/".to_string()` in if-else let binding |
| datagram_io/src/lib.rs | 267 | `"@".to_string()` in if-else let binding |
| datagram_io/src/lib.rs | 272 | `"@".to_string()` in if-else let binding |
| report_render_core/src/lib.rs | 330 | `group.code.clone()` in if-else let binding |
| hook_pre_subagent_bash/src/main.rs | 277 | `"  (none)".to_string()` in if-else let binding |

**Note on convert.rs:199**: `k.clone()` is inside `table.iter().map(|(k, v)| (k.clone(), ...))`. This should already be caught by `is_iterator_adaptor_closure`. If it's still flagged, the issue might be that the closure contains a tuple_expression, and the walk reaches the closure AFTER the tuple (transparent) but the closure check requires the closure to be the DIRECT parent of the arguments node. Investigate whether the walk correctly reaches the closure detection.

### The fix

In `clone_is_ownership_transfer()`, add `if_expression` and `else_clause` to the transparent wrapper list (line 421):

```rust
// Transparent wrappers -- keep walking up
"arguments" | "parenthesized_expression" | "reference_expression"
| "try_expression" | "type_cast_expression" | "assignment_expression"
| "tuple_expression" | "array_expression"
| "if_expression" | "else_clause"  // NEW: if-else branches produce owned values
=> {
    current = node.parent();
    continue;
}
```

Wait -- this is WRONG. Making `if_expression` transparent would skip ALL if-expressions, including ones where the clone is genuinely avoidable. The issue is specifically when the if-else is in a value-producing position (let binding initializer, return value, function argument).

**Better fix**: expand `is_implicit_return()` to recognize that a block inside an if-expression is a return position if the if-expression ITSELF is in a return position. This requires making `is_implicit_return` recursive:

```rust
fn is_value_producing_position(block: tree_sitter::Node, target: tree_sitter::Node) -> bool {
    match block.parent() {
        // Direct return contexts (existing logic)
        Some(p) if matches!(p.kind(), "function_item" | "match_arm" | "closure_expression") => {
            // target must be within the last expression of the block
            let mut cursor = block.walk();
            match block.named_children(&mut cursor).last() {
                Some(last) => {
                    target.start_byte() >= last.start_byte()
                        && target.end_byte() <= last.end_byte()
                }
                None => false,
            }
        }
        // NEW: if-else branches -- the block produces a value if the if-expression
        // is itself in a value-producing position. Walk up through the if/else chain.
        Some(p) if p.kind() == "if_expression" || p.kind() == "else_clause" => {
            // Find the outermost if_expression
            let mut if_expr = p;
            while let Some(parent) = if_expr.parent() {
                if parent.kind() == "else_clause" || parent.kind() == "if_expression" {
                    if_expr = parent;
                } else {
                    break;
                }
            }
            // The outermost if_expression must be in a position that consumes owned values
            matches!(
                if_expr.parent().map(|p| p.kind()),
                Some("let_declaration" | "assignment_expression" | "return_expression"
                     | "field_initializer" | "arguments" | "call_expression"
                     | "match_arm" | "tuple_expression" | "array_expression")
            )
        }
        _ => false,
    }
}
```

Then replace the `"block"` arm in `clone_is_ownership_transfer` to call this new function instead of `is_implicit_return`.

### Test cases to add

```rust
// FALSE POSITIVE -- must NOT flag
#[test]
fn to_string_in_if_else_let_binding_ok() {
    let code = r#"
        fn format_refs(refs: &[String]) -> String {
            let display = if refs.is_empty() {
                "none".to_string()
            } else {
                refs.join(", ")
            };
            display
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_string_abuse(&parsed, &default_config());
    assert!(violations.is_empty(), "to_string in if-else let binding should be skipped");
}

#[test]
fn clone_in_if_else_let_binding_ok() {
    let code = r#"
        fn pick(flag: bool, owned: &String, fallback: &String) -> String {
            let result = if flag {
                owned.clone()
            } else {
                fallback.clone()
            };
            result
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_clone_spam(&parsed, &default_config());
    assert!(violations.is_empty(), "clone in if-else let binding should be skipped");
}

#[test]
fn to_string_in_else_if_chain_ok() {
    let code = r#"
        fn normalize(path: &str) -> String {
            if path.starts_with("/") {
                format!("/{}", path)
            } else if path.is_empty() {
                ".".to_string()
            } else {
                path.to_string()
            }
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_string_abuse(&parsed, &default_config());
    assert!(violations.is_empty(), "to_string in else-if implicit return should be skipped");
}

// REAL VIOLATION -- must STILL flag
#[test]
fn to_string_in_plain_let_binding_still_caught() {
    let code = r#"
        fn process() {
            let name = "hello".to_string();
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_string_abuse(&parsed, &default_config());
    assert_eq!(violations.len(), 1, "to_string in plain let binding must still be caught");
}

#[test]
fn to_string_in_if_body_not_value_position_still_caught() {
    let code = r#"
        fn process(flag: bool) {
            if flag {
                let name = "hello".to_string();
            }
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_string_abuse(&parsed, &default_config());
    assert_eq!(violations.len(), 1, "to_string in if-body let binding (not value position) must still be caught");
}
```

---

## Change 2: Tier-aware no_println

### The gap

`check_no_println` (prohibited.rs line 239) skips `fn main()` and output functions (`print_*`, `emit_*`, `display_*`). It does NOT know about crate tiers. In nornir, all binary crates follow the pattern:

```rust
fn main() {
    let code = run();  // run() owns stdout
    process::exit(code);
}

fn run() -> Result<i32, String> {
    // ... actual work, including println! for output ...
    println!("{}", output);  // <-- flagged as violation
    Ok(0)
}
```

The `run()` function IS the binary's interface. `println!` inside it is intentional stdout output, not debug printing.

### Affected violations (4 of 29)

| File | Lines | Pattern |
|------|-------|---------|
| cli/syn_cli/src/main.rs | 340, 347, 356 | `println!` for formatted report output in `run()` |
| cli/saga_cli/src/main.rs | 182 | `println!` for JSON output in `run()` |

### The fix

`ParsedSource.file_path` is already available. Add a tier classifier for Rust files:

```rust
/// Nornir crate tier derived from file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustCrateTier {
    Core,       // core/     -- pure libraries, strictest checks
    Capability, // capability/ -- I/O libraries, strict checks
    Binary,     // cli/ hooks/ writers/ senders/ daemons/ interceptors/
    Gate,       // gates/    -- PyO3 boundary
    Unknown,    // anything else
}

fn classify_rust_tier(file_path: &str) -> RustCrateTier {
    // Normalize: look for the nornir directory markers
    if file_path.contains("/core/") {
        RustCrateTier::Core
    } else if file_path.contains("/capability/") {
        RustCrateTier::Capability
    } else if file_path.contains("/cli/")
        || file_path.contains("/hooks/")
        || file_path.contains("/writers/")
        || file_path.contains("/senders/")
        || file_path.contains("/daemons/")
        || file_path.contains("/interceptors/")
    {
        RustCrateTier::Binary
    } else if file_path.contains("/gates/") {
        RustCrateTier::Gate
    } else {
        RustCrateTier::Unknown
    }
}
```

Then in `check_no_println`, after the `fn main()` and output function checks, add:

```rust
// In binary crates, println! in the run() function is stdout output, not debug printing.
// Binary crates delegate from main() to run() — run() owns the stdout interface.
if name == "println" && classify_rust_tier(source.file_path) == RustCrateTier::Binary {
    if in_run_function(node, source.source_bytes) {
        continue;
    }
}
```

Where `in_run_function` is similar to `in_main_function` but checks for `run`:

```rust
fn in_run_function(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "function_item" {
            let name = ancestor
                .child_by_field_name("name")
                .map(|n| node_text(n, source))
                .unwrap_or("");
            return name == "run";
        }
        current = ancestor.parent();
    }
    false
}
```

**Design choice**: This is deliberately narrow — only `fn run()` in binary crates, not a blanket "skip all println in binaries." A `println!` in a helper function inside a binary crate IS a debug print that should use `eprintln!` or be removed. Only `main()` and `run()` own stdout.

### Alternative considered and rejected

Making `run` a general output function name (adding to the `in_output_function` list) would suppress `println!` in `fn run()` everywhere, including library code. Tier-awareness keeps the suppression scoped to binary crates where the convention applies.

### Test cases to add

```rust
// FALSE POSITIVE -- must NOT flag
#[test]
fn println_in_run_function_binary_crate_ok() {
    let source: &'static [u8] = Box::leak(
        b"fn run() -> Result<i32, String> { println!(\"output\"); Ok(0) }"
            .to_vec().into_boxed_slice(),
    );
    let parsed = build_parsed_source_rust("/nornir/cli/syn_cli/src/main.rs", source).unwrap();
    let violations = check_no_println(&parsed, &default_config());
    assert!(violations.is_empty(), "println in run() of binary crate should be allowed");
}

// REAL VIOLATION -- must STILL flag
#[test]
fn println_in_run_function_library_crate_caught() {
    let source: &'static [u8] = Box::leak(
        b"fn run() { println!(\"debug\"); }"
            .to_vec().into_boxed_slice(),
    );
    let parsed = build_parsed_source_rust("/nornir/core/some_core/src/lib.rs", source).unwrap();
    let violations = check_no_println(&parsed, &default_config());
    assert_eq!(violations.len(), 1, "println in run() of library crate must still be caught");
}

#[test]
fn println_in_helper_function_binary_crate_caught() {
    let source: &'static [u8] = Box::leak(
        b"fn helper() { println!(\"debug\"); }\nfn run() -> i32 { helper(); 0 }"
            .to_vec().into_boxed_slice(),
    );
    let parsed = build_parsed_source_rust("/nornir/cli/syn_cli/src/main.rs", source).unwrap();
    let violations = check_no_println(&parsed, &default_config());
    assert_eq!(violations.len(), 1, "println in helper within binary crate must still be caught");
}

#[test]
fn dbg_in_run_function_binary_crate_still_caught() {
    let source: &'static [u8] = Box::leak(
        b"fn run() -> i32 { let x = 1; dbg!(x); 0 }"
            .to_vec().into_boxed_slice(),
    );
    let parsed = build_parsed_source_rust("/nornir/cli/syn_cli/src/main.rs", source).unwrap();
    let violations = check_no_println(&parsed, &default_config());
    assert_eq!(violations.len(), 1, "dbg! must always be caught, even in binary run()");
}
```

### Tier classifier tests

```rust
#[test]
fn tier_classification() {
    assert_eq!(classify_rust_tier("/nornir/core/text_core/src/lib.rs"), RustCrateTier::Core);
    assert_eq!(classify_rust_tier("/nornir/capability/hook_io/src/lib.rs"), RustCrateTier::Capability);
    assert_eq!(classify_rust_tier("/nornir/cli/syn_cli/src/main.rs"), RustCrateTier::Binary);
    assert_eq!(classify_rust_tier("/nornir/hooks/hook_pre_llm_bash/src/main.rs"), RustCrateTier::Binary);
    assert_eq!(classify_rust_tier("/nornir/writers/append_raw_jsonl/src/main.rs"), RustCrateTier::Binary);
    assert_eq!(classify_rust_tier("/nornir/gates/gate_structure_in/src/lib.rs"), RustCrateTier::Gate);
    assert_eq!(classify_rust_tier("/other/project/src/main.rs"), RustCrateTier::Unknown);
}
```

---

## Change 3: Remaining clone/string violations that are NOT if-else

After Changes 1 and 2, some violations remain that are genuinely hard:

### report_render_core:47 — clone into BTreeMap key

```rust
let key = (issue.tool.clone(), issue.code.clone());
```

Walk: clone -> tuple_expression (transparent) -> let_declaration -> not recognized -> false.

This IS a necessary clone (building an owned key for a BTreeMap). The detector would need data-flow analysis to know the let binding feeds into a map operation. **Recommendation: leave as-is.** These are genuine ownership patterns that are too context-dependent for AST analysis. The 3 violations in report_render_core are documented as known deferred work.

### diff_core:112 — clone-to-mutate

```rust
let mut stripped = value.clone();
remove_transient_fields(&mut stripped);
```

Cloning a `&Value` to create a mutable copy. The borrow prevents in-place mutation. This is architecturally correct — `strip_transient` takes `&Value` because it must not modify the original.

**Recommendation**: add `let_declaration` detection only when the let binding has `mut` keyword. A `let mut x = val.clone()` is almost always intentional — you need a mutable copy. A `let x = val.clone()` (without mut) is more likely avoidable.

```rust
// In clone_is_ownership_transfer, add before the catch-all:
"let_declaration" => {
    // let mut x = val.clone() -- cloning to mutate is intentional
    let text = node_text(node, source);
    if text.contains("let mut") {
        return true;
    }
    return false;
}
```

This catches diff_core:112 specifically. Test case:

```rust
#[test]
fn clone_into_let_mut_ok() {
    let code = r#"
        fn strip(value: &serde_json::Value) -> serde_json::Value {
            let mut stripped = value.clone();
            stripped
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_clone_spam(&parsed, &default_config());
    assert!(violations.is_empty(), "clone into let mut should be skipped");
}

#[test]
fn clone_into_let_immutable_still_caught() {
    let code = r#"
        fn copy(value: &String) {
            let copied = value.clone();
        }
    "#;
    let parsed = parse(code);
    let violations = check_no_clone_spam(&parsed, &default_config());
    assert_eq!(violations.len(), 1, "clone into immutable let should still be caught");
}
```

### diff_core:182 — clone-before-overwrite

```rust
let prev = existing_val.clone();
*existing_val = Value::Array(vec![prev, value]);
```

Same as clone-to-mutate: `let mut` isn't present here, but the let binding is used in the very next line. This won't be caught by the `let mut` heuristic. **Recommendation: leave as-is.** This is 1 violation, and fixing the detector to understand "clone used in the next statement" requires control-flow analysis.

### format_core/tomlx/annotation.rs:176 — clone in FieldLineParse::Error

```rust
FieldLineParse::Error {
    field_name,
    raw: comment.to_string(),
    reason,
}
```

Wait — `field_name` (line 176) and `comment.to_string()` (line 185) are in a struct literal. The `field_initializer` case should catch these. If `field_name` is being flagged as a clone, it might be a `field_name.clone()` happening elsewhere. Need Tyr to verify the exact AST at that line. If it IS in a struct literal, the detection has a bug in field_initializer recognition.

### format_core/tomlx/types.rs:237 — self.data.clone() in to_json()

```rust
pub fn to_json(&self) -> serde_json::Value {
    let mut output = self.data.clone();
    // ... mutates output ...
}
```

This is the clone-to-mutate pattern. Will be fixed by the `let mut` detection in Change 3.

### format_core/tomlx/validation.rs:128 — annotation.path_bases.clone()

```rust
state.path_registry.user_defined = annotation.path_bases.clone();
```

Walk: clone -> assignment_expression (transparent) -> ... The assignment_expression IS in the transparent list already. The walk should continue up to whatever contains the assignment. If the assignment is a statement in a block, it hits `block` → `is_implicit_return` → probably not the last expression → false.

But this IS an ownership transfer — assigning into a struct field via `state.path_registry.user_defined = ...`. The `assignment_expression` is transparent, so the walk goes up to... the expression_statement wrapping it? Or directly to the block?

**Recommendation**: Tyr should add a test case for this exact pattern and trace the AST walk. If `assignment_expression` transparency is working correctly, the parent after the assignment should be `expression_statement` (which is not recognized). Adding `expression_statement` as transparent might help, but could also over-suppress. Needs investigation.

---

## Summary: Expected impact

| Change | Violations resolved | Violations remaining |
|--------|--------------------:|---------------------:|
| 1. if-else branch ownership | ~10 | |
| 2. Tier-aware no_println | 4 | |
| 3. let-mut clone detection | 2 | |
| Still legitimately flagged | | ~8 |
| Known deferred (report_render_core) | | 3-4 |
| Needs investigation | | 1-2 |
| **Total** | **~16** | **~13** |

After these changes, the remaining ~13 violations should be genuine issues that need code fixes, not detection improvements.

---

## Architecture note: RustCrateTier

The tier classifier is useful beyond `no_println`. Future applications:

- **no_clone_spam**: Core crates could have stricter thresholds (no clone at all in pure functions?)
- **function_length**: Binary crates might get a slightly higher threshold for error-formatting match arms
- **no_pub_overuse**: Already uses filename-based exemption; tier could replace this

Recommend placing `classify_rust_tier` and `RustCrateTier` in a shared location (perhaps `classify.rs` alongside `classify_file`) so all Rust checks can use it.

---

## Files to modify

1. `checks_rs/prohibited.rs` — Changes 1, 2, 3 (ownership detection, tier println, let-mut)
2. `classify.rs` or new `classify_rust.rs` — RustCrateTier enum and classifier
3. `structures.rs` — no changes needed (CheckConfig already has extensible fields)
4. Test additions in both `prohibited.rs` and the classify module

## Build verification

After changes, run:
```
cargo test -p gleipnir_core
```

Then rebuild and re-scan:
```
nornir_deploy --build gates
syn ~/ai/smidja/nornir
```

The violation count should drop from 29 to ~13, with zero new regressions (no previously-clean code becoming flagged).
