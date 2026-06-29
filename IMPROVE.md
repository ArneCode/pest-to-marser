# Improvements

- Hoist ASCII builtins into local bindings instead of inlining them everywhere.
  For example, when `ASCII_ALPHA`, `ASCII_ALPHANUMERIC`, `ASCII_HEX_DIGIT`, etc. are used in the grammar, generate a binding such as `let ASCII_ALPHANUMERIC = ...;` first and reference it from expressions. This should reduce repeated `one_of(...)` expansions and make generated code easier to read.

- Add a helper for case-insensitive strings.
  Current lowering expands each ASCII letter into a per-character `one_of((lower, upper))`, which gets long quickly. A generated helper or marser helper for case-insensitive literal matching would keep Pest expressions like `^"select"` compact in generated Rust.

- Use marser's bounded matcher repetition helper once available.
  Marser is adding `repeat` in `src/matcher/repeat.rs` for bounded matcher repetition, with forms like `repeat(m, 2..5)`, `repeat(m, 1..)`, `repeat(m, 2..=5)`, and `repeat(m, n)`. Use this instead of expanding bounded Pest repeats into explicit tuples and nested `optional(...)` calls.
