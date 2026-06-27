# To Fix

All items below were addressed. `cargo test` passes (11 lib tests + 8 integration tests).

## 1. Generated Code Mixes Parsers Into Matcher Positions — FIXED

Rule bodies now always emit matcher tuples inside `capture!((...) => ())`. Literals, ranges, and builtins use matcher forms; rule references use `.ignore_result()`.

## 2. `leading_choice` Normalization Is Wrong With Infix Operators — FIXED

Infix operators are folded first; a leading `|` prepends `Empty` to the folded result (flattening nested `Choice` when needed).

## 3. User-Defined Builtin Names Do Not Win — FIXED

Identifiers normalize to `RuleRef` first; `resolve_builtins` runs after the rule table is built and only promotes unresolved names to `Builtin`.

## 4. Finite Repetition Validation And Lowering Are Too Rough — FIXED

Validation applies only to unbounded repetitions (`*`, `+`, `{n,}`). Finite `{n}`, `{,n}`, and `{m,n}` skip the infinite-loop check. Codegen inserts `ws` between repeated items for ranged and optional repetitions in `NormalWs` context.

## 5. Emission Ordering Can Reference Bindings Before Definition — FIXED

Emission follows SCC topological order. Before the first `NormalWs` symbol, `emit_ws_prerequisites` emits `WHITESPACE`/`COMMENT` and their dependencies, then `ws`. Normal-context rules depend on ws-unit symbols for ordering.
