# pest-to-marser

Convert [Pest](https://pest.rs/) grammars into [Marser](https://crates.io/crates/marser) parser combinators in Rust.

The generated output is a single `grammar()` function that returns a Marser parser. For the chosen entry rule, the converter targets strict language equivalence with Pest: the same complete inputs are accepted or rejected. Output type is `()` everywhere — parse trees, spans, and tags are not generated.

**Try it in the browser:** [https://pest-to-marser.arnedebo.com](https://pest-to-marser.arnedebo.com) — paste a grammar, preview the Rust output, download a Cargo project, or share a link.

## Install

```bash
cargo install pest-to-marser
```

Or build from this repository:

```bash
cargo install --path .
```

## Usage

```bash
pest-to-marser <grammar.pest> [entry_rule] [--output <path>] [--trace]
```

| Argument | Description |
|----------|-------------|
| `grammar.pest` | Path to the Pest grammar file |
| `entry_rule` | Rule to use as the parser entry point (defaults to the last rule in the file) |
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
pest-to-marser calc.pest expr
```

The output is Rust source defining `pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone`. Use it with Marser's `Parser::parse_str` or `parse_whole_input`:

```rust
use marser::parser::Parser;

let parser = grammar();
parser.parse_whole_input("1 + 2 * 3").unwrap();
```

## Library API

The conversion logic is also available as a library:

```rust
use pest_to_marser::{ConvertOptions, convert_pest_source};

let rust = convert_pest_source(
    pest_source,
    &ConvertOptions {
        entry_rule: "expr".to_string(),
        ..Default::default()
    },
)?;
```

Other exports include `convert_pest_grammar`, `list_pest_rules`, and `get_pest_grammar` for working with the parsed grammar AST.

## Supported Pest features

The converter handles a practical subset of Pest, including:

- Rule modifiers: silent (`_`), atomic (`@`), compound atomic (`$`), non-atomic (`!`)
- Sequences, ordered choice, optional (`?`), repetition (`*`, `+`, `{m,}`, `{m}`), and negative lookahead (`!`)
- Positive lookahead (`&`) and case-insensitive strings (`^"..."`)
- Builtins: `ASCII_ALPHA`, `ASCII_ALPHANUMERIC`, `ASCII_DIGIT`, `ANY`, `SOI`, `EOI`
- Implicit whitespace via `WHITESPACE` and `COMMENT` rules
- Right recursion and mutual recursion (via Marser's `recursive` helpers)

## Limitations

The following are **not** supported and produce conversion errors:

- Left recursion
- Pest stack features (`PUSH`, `POP`, `DROP`, `PEEK`, `PEEK_ALL`)
- `Pair` / `Rule` enum output, spans, tags, or token preservation
- Memoization (`.memoized()`)
- Mutual recursion groups larger than Marser's `recursive12` limit

Repetition and whitespace rules are validated with Pest-style progress checks. Unsupported constructs that could affect matching semantics are hard errors, not warnings.

## Development

```bash
# Run tests (includes equivalence checks against Pest for fixture grammars)
cargo test

# Regenerate committed output snapshots after converter changes
cargo run --bin update-test-fixtures
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
