# grammar-to-marser: conversion plan

This document captures the current design for converting pest grammars into marser parser code.

## Goal

Build a tool that:

1. Parses a pest grammar file using the marser meta-grammar in `src/grammar.rs`.
2. Validates and normalizes the grammar into a semantic representation.
3. Resolves pest matching semantics: rule references, implicit whitespace, modifiers, recursion, and builtins.
4. Emits marser grammar code as Rust source.

**v1 target:** strict language equivalence for a useful subset of pest. For the chosen entry rule, the generated parser should accept and reject the same complete inputs as pest. Output type is `()` everywhere. Pest-like parse tree shape is out of scope initially.

The entry rule is explicit. The CLI/user supplies it; do not infer it from `SOI` / `EOI` usage or from rule order.

## Non-goals (v1)

- `PUSH` / `POP` / `DROP` / `PEEK` / `PEEK_ALL` stack features.
- Pest `Pair` output, `Rule` enum output, spans, tags, or token preservation.
- Error message parity with pest.
- Generated helper functions per rule.
- Generated `.memoized()` calls.

Unsupported constructs that can affect matching are hard conversion errors, not warnings.

---

## Pipeline

```
pest source
  -> strict AST parse (parse_pest_grammar + end_of_input)
  -> rule table + duplicate-name validation
  -> explicit entry rule validation
  -> raw AST -> normalized Expr tree
  -> builtin / unsupported feature validation
  -> progress analysis and pest-style repetition validation
  -> WHITESPACE / COMMENT validation
  -> left-recursion validation
  -> effective context propagation (NormalWs / AtomicNoWs)
  -> specialized symbol graph
  -> SCC analysis (Tarjan) + condensation topo order
  -> marser codegen (one grammar() function)
```

The current parser AST may stay as the syntax-level representation. All semantic passes and codegen should use the normalized expression tree.

---

## Normalized expression tree

The raw AST stores expressions as flat `terms` + `infix_ops`. Normalize this into a semantic tree before validation and codegen.

Pest precedence, low to high:

1. `|` choice
2. `~` sequence
3. prefix `&` / `!` predicates
4. postfix `?` `*` `+` `{n...}`
5. grouping

Suggested normalized shape:

```rust
enum Expr {
    Empty,
    Builtin(Builtin),
    RuleRef(String),
    Literal(String),
    InsensitiveLiteral(String),
    Range { start: char, end: char },
    Sequence(Vec<Expr>),
    Choice(Vec<Expr>),
    Prefix { op: PrefixOp, expr: Box<Expr> },
    Postfix { expr: Box<Expr>, op: PostfixOp },
}
```

`leading_choice: true` becomes an empty alternative: `| a | b` is `Choice([Empty, a, b])`.

Tags can be preserved in the normalized tree if useful for future phases, but v1 output is `()`, so tags do not affect matching.

---

## Core difficulty: pest semantics vs marser

Pest has implicit whitespace and context-sensitive rule behavior. Marser requires explicit whitespace parsers and has no pest atomic/silent primitives.

### Matching contexts

For v1, track only matching behavior, not output/token behavior. There are two effective contexts:

| Context | Meaning |
|---------|---------|
| `NormalWs` | implicit whitespace is active where pest inserts it |
| `AtomicNoWs` | implicit whitespace is disabled |

Rule modifiers map to v1 matching contexts as follows:

| Pest modifier | v1 matching behavior |
|---------------|----------------------|
| none | inherit caller context |
| `_` silent | inherit caller context; output suppression is irrelevant for `()` |
| `@` atomic | force `AtomicNoWs` and cascade into normal/silent callees |
| `$` compound atomic | same as `@` for v1 accept/reject behavior |
| `!` non-atomic | force `NormalWs`, stopping atomic cascade |

`$` and `@` differ for pest token output, but not for v1 language equivalence.

### Implicit whitespace

- Inserted only if the grammar defines `WHITESPACE` and/or `COMMENT`.
- Inserted only in `NormalWs`.
- Inserted between `~` sequence operands.
- Inserted between repetitions of `*`, `+`, and ranged repetitions when the repeated item appears more than once.
- Not inserted at the start or end of a rule body.
- Not automatically inserted inside the generated `ws()` loop itself.

`WHITESPACE` and `COMMENT` are unit rules. Pest repeats them implicitly. Generate a synthetic whitespace parser equivalent to:

```rust
many(one_of((whitespace_unit, comment_unit)))
```

Do not lower a `WHITESPACE` / `COMMENT` rule as though it already consumes unbounded whitespace unless the user explicitly wrote that, and reject definitions that can succeed without consuming input.

### Repetition + whitespace

| Pest | Marser sketch |
|------|----------------|
| `a ~ b` | `capture!((a, ws(), b) => ())` when `NormalWs` |
| `a ~ b` | `capture!((a, b) => ())` when `AtomicNoWs` |
| `item*` | zero matches or `item (ws item)*`; no leading/trailing ws |
| `item+` | `item (ws item)*`; no leading/trailing ws |
| `item?` | `optional(item)` |
| `{n}` / `{n,}` / `{,n}` / `{m,n}` | generate explicit repetition helpers or expansions with the same whitespace-between-items rule |

### Feature mapping

| Pest | Marser |
|------|--------|
| `~` | sequence, with explicit `ws()` between operands in `NormalWs` |
| `|` | ordered `one_of((...))` |
| `&` / `!` prefix | `positive_lookahead` / `negative_lookahead` |
| `^"..."` | ASCII-only case-insensitive matcher |
| `'a'..'z'` | `'a'..='z'` |
| `ANY` | `AnyToken` |
| `SOI` | `start_of_input()` |
| `EOI` | `end_of_input()` |

Avoid `commit_on` in generated code unless proven semantics-preserving. Pest uses ordered choice with backtracking.

---

## Builtins

Treat supported pest builtins as reserved symbols unless the user defines a rule with the same name. User-defined rules win.

Supported v1 builtins:

- `SOI`
- `EOI`
- `ANY`

Also support common ASCII builtins by injecting/lowering synthetic rules only when referenced:

- `NEWLINE`
- `ASCII_DIGIT`
- `ASCII_NONZERO_DIGIT`
- `ASCII_BIN_DIGIT`
- `ASCII_OCT_DIGIT`
- `ASCII_HEX_DIGIT`
- `ASCII_ALPHA_LOWER`
- `ASCII_ALPHA_UPPER`
- `ASCII_ALPHA`
- `ASCII_ALPHANUMERIC`

Unknown builtins and unsupported grammar-extras are hard conversion errors.

---

## Progress and recursion validation

The converter should match pest's validator intent, not merely rely on marser runtime safeguards.

### Progress analysis

Compute expression properties such as:

- `can_match_empty`
- `can_succeed_without_progress`
- `must_consume_on_success`

Use these to reject repeated expressions that can succeed without consuming in a way that makes repetition unsafe. Marser's `many` and `one_or_more` stop on no progress, but generated parsers should represent pest-valid grammars only.

Always reject `WHITESPACE` and `COMMENT` rules that can succeed without consuming input, because pest implicitly repeats them.

### Left recursion

Reject direct and indirect left recursion in v1. Report the recursion cycle when possible, for example `expr -> term -> expr`.

Right recursion and non-left mutual recursion can be supported through marser `recursive` / `recursiveN`.

---

## Specialization strategy

After normalization and validation, propagate effective matching context from the explicit entry rule.

Specialized graph nodes are `(rule_name, context)` where context is `NormalWs` or `AtomicNoWs`, unless the rule's definition modifier forces one context:

- none / `_`: inherit call-site context
- `@` / `$`: force `AtomicNoWs`
- `!`: force `NormalWs`

Generate only the specialized variants that are reachable from the chosen entry rule. If a normal rule is used only from atomic context, only its atomic variant is needed.

Equivalent trivial variants may be collapsed later as an optimization, but correctness should come from context propagation, not from heuristic duplication.

At call sites, reference the correct specialized binding directly. Do not generate a runtime context parameter.

---

## Rule graph and SCCs

Run Tarjan on the specialized symbol graph. The condensation graph is a DAG and should be emitted in dependency order.

| SCC shape | Codegen |
|-----------|---------|
| Single node, no self-loop | Plain local `let` binding |
| Single node, self-loop | `recursive` |
| Size 2 through 12 | `recursive2` through `recursive12` |
| Size greater than 12 | hard conversion error in v1 |

Mutual recursion cannot be split into unrelated top-level definitions. All members of a cyclic SCC must be made available through recursive handles.

For SCCs larger than marser's `recursive12`, report a clear v1 limitation. Nested recursive-block codegen may be explored later, but it is not part of the initial implementation.

---

## Codegen shape: one `grammar()` function

v1 emits one public function per converted pest grammar:

```rust
pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    // 1. synthetic builtin helpers, if referenced
    // 2. ws / comment unit parsers and synthetic ws()
    // 3. acyclic specialized symbols needed by recursive bodies
    // 4. recursive SCC blocks
    // 5. acyclic specialized symbols that depend on recursive symbols
    // 6. exact selected entry rule
}
```

`grammar()` should represent the exact chosen pest entry rule. It should not automatically wrap the entry rule in `SOI` / `EOI`; marser's `Parser::parse_str` and `parse_whole_input` already enforce whole-input parsing for callers that use those APIs. If the pest grammar explicitly contains `SOI` or `EOI`, lower them as ordinary anchors.

### Practical codegen policy

- Emit local bindings inside `grammar()` for v1.
- Use `.erase_types()` proactively at generated rule / specialized-symbol boundaries if it compiles cleanly with marser.
- Do not emit `.memoized()` anywhere in v1.
- Sanitize generated Rust identifiers, including Rust keywords and duplicate names introduced by specialization.
- Preserve `//!` / `///` docs as Rust comments when practical.

Helper functions per rule are a future readability/compile-time optimization, not part of v1.

---

## Validation

Accumulate validation errors where practical and report them together. Stop a phase early only when later analysis would be ambiguous or misleading.

Validation should cover:

- duplicate rule names
- missing or unknown explicit entry rule
- undefined rule references
- supported builtins vs unsupported builtins
- unsupported stack constructs and other unsupported pest features
- invalid or non-progressing repetitions
- nullable `WHITESPACE` / `COMMENT`
- left-recursion cycles
- reachability from the explicit entry rule

Warnings:

- specialization split a rule into multiple contexts
- suspicious but valid `WHITESPACE` / `COMMENT` definitions that consume more than one unit per invocation

---

## Testing strategy

Layered tests:

1. **Meta-parser:** pest grammar parses to AST and consumes all input.
2. **Normalization:** flat `terms` + `infix_ops` become the expected `Expr` tree.
3. **Builtin validation/lowering:** supported builtins and injected ASCII helpers.
4. **Whitespace pass:** exact insertion positions for small expressions.
5. **Progress validation:** unsafe repeats and nullable `WHITESPACE` / `COMMENT`.
6. **Left-recursion validation:** direct and indirect cycles.
7. **Specialization:** rules reached in normal and atomic contexts.
8. **SCC/codegen:** recursive and mutually recursive clusters, including nested `recursiveN`.
9. **Generated-code compile tests:** small grammars compile as Rust.
10. **End-to-end equivalence:** converted grammar vs pest on accept/reject corpora.

Start with tiny fixtures before JSON/calc-size grammars.

---

## v1 scope checklist

- [x] Parse pest grammar -> raw AST
- [ ] Require full input consumption in `parse_pest_grammar`
- [ ] Rule table + duplicate-name validation
- [ ] Explicit entry rule handling
- [ ] Raw AST -> normalized `Expr`
- [ ] Builtin classification and synthetic ASCII builtins
- [ ] Unsupported feature validation
- [ ] Progress analysis and repetition validation
- [ ] Left-recursion validation
- [ ] Strict `WHITESPACE` / `COMMENT` validation and lowering
- [ ] Two-context modifier model (`NormalWs`, `AtomicNoWs`)
- [ ] Specialization analysis from the explicit entry rule
- [ ] SCC analysis on specialized graph
- [ ] Single-function codegen with `.erase_types()` boundaries
- [ ] Hard error for specialized SCCs larger than 12
- [ ] Default output type `()`
- [ ] End-to-end equivalence tests

## Later phases

- Typed output.
- Tags and captures.
- Pest-like spans / `Pair` tree shape.
- Helper functions per generated rule.
- Selective `.memoized()` for performance.
- Nested recursive-block codegen for SCCs larger than 12.
- Broader grammar-extras / Unicode builtin support.
