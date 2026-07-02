# grammar-to-marser

Convert grammars (currently [Pest](https://pest.rs/) and a supported PEG subset) into [Marser](https://crates.io/crates/marser) parser combinators in Rust.

The generated output is a single `grammar()` function that returns a Marser parser. For the chosen entry rule, the converter targets strict language equivalence with the selected syntax where implemented. The parser output type is a generated `Parsed<'src>` enum with one variant per rule.

**Try it in the browser:** [https://grammar-to-marser.arnedebo.com](https://grammar-to-marser.arnedebo.com) — paste a grammar, preview the Rust output, download a Cargo project, or share a link.

> **Note:** This project was almost entirely vibe coded. If you run into bugs or rough edges, feel free to [open an issue](https://github.com/ArneCode/grammar-to-marser/issues).

## Install

```bash
cargo install grammar-to-marser
```

Or build from this repository:

```bash
cargo install --path .
```

## Usage

```bash
grammar-to-marser <grammar-file> [entry_rule] [--syntax pest|peg] [--output <path>] [--trace]
```

| Argument | Description |
|----------|-------------|
| `grammar-file` | Path to the grammar file |
| `entry_rule` | Rule to use as the parser entry point (defaults to the last rule in the file) |
| `--syntax pest\|peg` | Input grammar syntax (defaults to `pest`) |
| `--output <path>` | Write generated Rust to a file instead of stdout |
| `--trace` | Emit trace instrumentation in the generated parser |

### Example

Given `calc.pest`:

```pest
expr = { term ~ (("+" | "-") ~ term)* }
term = { factor ~ (("*" | "/") ~ factor)* }
factor = { number | "(" ~ expr ~ ")" }
number = @{ ASCII_DIGIT+ }
WHITESPACE = _{ " " | "\t" }
```

Generate a parser for the `expr` rule:

```bash
grammar-to-marser calc.pest expr --syntax pest
```

The output is Rust source defining `pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone`, plus a `Parsed<'src>` enum. Each rule becomes a variant; fields come from binds inside that rule. Nested rule values are `Box<Parsed<'src>>`; leaf rules capture their matched slice as `&'src str`. Use it with Marser's `Parser::parse_str` or `parse_whole_input`:

```rust
use marser::parser::Parser;

let parser = grammar();
let parsed = parser.parse_whole_input("1 + 2 * 3").unwrap();
```

## Library API

The conversion logic is also available as a library:

```rust
use grammar_to_marser::{ConvertOptions, convert_grammar_source};

let rust = convert_grammar_source(
    pest_source,
    &ConvertOptions {
        entry_rule: "expr".to_string(),
        ..Default::default()
    },
)?;
```

Other exports include `convert_source`, `list_rules`, `convert_grammar`, `list_grammar_rules`, `parse_pest_grammar`, and `parse_peg_grammar`.

## Supported Pest features

The converter handles a practical subset of Pest, including:

- Rule modifiers: silent (`_`), atomic (`@`), compound atomic (`$`), non-atomic (`!`)
- Sequences, ordered choice, optional (`?`), repetition (`*`, `+`, `{m,}`, `{m}`), and negative lookahead (`!`)
- Positive lookahead (`&`) and case-insensitive strings (`^"..."`)
- Builtins: `ASCII_ALPHA`, `ASCII_ALPHANUMERIC`, `ASCII_DIGIT`, `ANY`, `SOI`, `EOI`
- Implicit whitespace via `WHITESPACE` and `COMMENT` rules
- Right recursion and mutual recursion (via Marser's `recursive` helpers)
- Typed `Parsed<'src>` output with one enum variant per rule
- Pest node tags as field names; tagged non-rule matchers become `&'src str` slices

## Limitations

The following are **not** supported and produce conversion errors:

- Left recursion
- Pest stack features (`PUSH`, `POP`, `DROP`, `PEEK`, `PEEK_ALL`)
- Pest-style `Pair` trees, spans, or memoization (`.memoized()`)
- Mutual recursion groups larger than Marser's `recursive12` limit

Repetition and whitespace rules are validated with Pest-style progress checks. Unsupported constructs that could affect matching semantics are hard errors, not warnings.

## Development

```bash
# Run tests (includes equivalence checks against Pest for fixture grammars)
cargo test

# Regenerate committed output snapshots after converter changes
cargo run --features dev-tools --bin update-test-fixtures
```

The `web/` crate is a WASM build of the converter used by the browser demo.

## How it works

```
Pest source
  → parse with Marser meta-grammar
  → normalize & validate (whitespace, recursion, repetitions, builtins)
  → specialize rule contexts (atomic vs non-atomic)
  → SCC analysis for recursive rules
  → emit Marser parser combinators
```

See [PLAN.md](PLAN.md) for the full design document.
