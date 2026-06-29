export const EXAMPLES = {
  simple: {
    label: "Simple",
    description: "Key/value list with whitespace, comments, and SOI/EOI.",
    entryRule: "main",
    pest: `WHITESPACE = _{ " " | "\\t" | newline }
COMMENT = _{ line_comment }
newline = _{ "\\n" | "\\r\\n" }
line_comment = _{ "//" ~ (!newline ~ ANY)* }

main = { SOI ~ item ~ ("," ~ item)* ~ EOI }
item = { ident ~ "=" ~ number }
ident = @{ ("_" | ASCII_ALPHA) ~ ("_" | ASCII_ALPHANUMERIC)* }
number = @{ ASCII_DIGIT+ }
`,
  },
  calc: {
    label: "Calculator",
    description: "Arithmetic with precedence — entry rule expr, no explicit whitespace rule.",
    entryRule: "expr",
    pest: `expr = { term ~ (("+" | "-") ~ term)* }
term = { factor ~ (("*" | "/") ~ factor)* }
factor = { number | "(" ~ expr ~ ")" }
number = @{ ASCII_DIGIT+ }
WHITESPACE = _{ " " | "\\t" }
`,
  },
  optional: {
    label: "Optional",
    description: "Block with optional trailing semicolon — shows ? repetition.",
    entryRule: "main",
    pest: `WHITESPACE = _{ " " }
main = { SOI ~ "{" ~ ident ~ (";" ~ ident)* ~ ";"? ~ "}" ~ EOI }
ident = @{ ("_" | ASCII_ALPHA) ~ ("_" | ASCII_ALPHANUMERIC)* }
`,
  },
};

export const DEFAULT_PEST = EXAMPLES.simple.pest;
